"""
Anti Timpa QRIS - Mobile App (dual-layer: Layer 1 OPTIK + Layer 2 EMVCo + kamera)

Mode analisis:
* DUAL-LAYER + KAMERA NYATA (utanpa sintetik) -- pakai bridge Camera4Kivy:
  - Camera4Kivy (Preview) menyediakan frame RGBA dari kamera Android (CameraX)
    via `analyze_pixels_callback()`.
  - Frame RGBA -> BGR -> `QrisScannerCore.process_frame()` yang menjalankan
    Layer 1 (layer1_optical: edge density + glare variance) DAN Layer 2
    (layer2_emvco: payload EMVCo + CRC-16), semuanya LOKAL di perangkat.
  - HUD bbox digambar lalu dirender ke Preview lewat grafika Kivy.

* DUAL-LAYER + SINTETIK (fallback bila kamera/provider tidak tersedia):
  generator frame sintetik yang menyuntikkan anomali stiker/glare periodik,
  supaya pipeline Layer 1 tetap dieksekusi di perangkat (demo nyata).

* LAYER-2-ONLY (bilamana OpenCV/numpy tidak ter-paket, APK ringan):
  tempel payload QRIS lalu Analisis; 100% pure-Python (layer2_emvco).
"""

import os
import sys
import threading
import traceback
from datetime import datetime

# ---- Make analysis modules importable regardless of CWD ----
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

# --------------------------------------------------------------------------
# Crash / error logging yang PERSISTEN (tetap tersimpan walau app langsung
# tertutup). Berguna saat app crash di Android dan kita tak bisa melihat log
# cukup cepat di device. File log ditaruh di app private storage agar mudah
# diambil via adb:  adb run-as org.antitimpa.antitimpa cat files/antitimpa.log
# --------------------------------------------------------------------------
LOG_FILENAME = "antitimpa.log"


def _log_path():
    """Lokasi file log. Di Android pakai app private storage; di desktop CWD."""
    try:
        from android.storage import app_storage_path
        return os.path.join(app_storage_path(), LOG_FILENAME)
    except Exception:
        return os.path.join(os.getcwd(), LOG_FILENAME)


LOG_PATH = None


def log_crash(kind, text, e=None, tb=None, always_print=False):
    """Tulis traceback/error ke file log (sebagai append) + ke stdout."""
    global LOG_PATH
    try:
        if LOG_PATH is None:
            LOG_PATH = _log_path()
            try:
                # buat header log baru hanya sekali per proses
                if not os.path.exists(LOG_PATH) or os.path.getsize(LOG_PATH) == 0:
                    with open(LOG_PATH, "a") as f:
                        f.write("==== Anti Timpa log session %s ====\n" % datetime.now())
            except Exception:
                pass
        lines = [
            "[%s] %s" % (datetime.now().isoformat(), kind),
            text,
        ]
        if e is not None:
            lines.append("Tipe: %s" % type(e).__name__)
            lines.append("Pesan: %s" % str(e))
        if tb is not None:
            lines.append("".join(traceback.format_tb(tb)))
        msg = "\n".join(lines)
        with open(LOG_PATH, "a") as f:
            f.write("\n" + msg + "\n")
        if always_print:
            print(msg, flush=True)
        return msg
    except Exception:
        if always_print:
            print(text, flush=True)
        return text


def _log_to_adb(tag, msg):
    """Kirim pesan log ke logcat (sumber adb) dengan tag yang mudah difilter.

    Di Android memakai `android.util.Log` (via pyjnius) supaya muncul dengan tag
    `ANTITIMPA` dan bisa dibaca via:  adb logcat -s ANTITIMPA
    Di luar Android (desktop/Python biasa) fallback ke print, yang tetap masuk
    stdout (lihat README).
    """
    try:
        from jnius import autoclass
        android_log = autoclass("android.util.Log")
        android_log.d(tag, str(msg))
        return
    except Exception:
        pass
    try:
        print("[%s] %s" % (tag, msg), flush=True)
    except Exception:
        pass


def log_scan_detail(snap):
    """Bangun string log detail dari snapshot dan tulis ke: (1) file antitimpa.log,
    (2) logcat adb (tag ANTITIMPA). Dipanggil tiap kali hasil scan diperbarui.
    Mengembalikan string detail (untuk ditampilkan di UI)."""
    l1 = snap.get('l1', {})
    l2 = snap.get('l2', {})
    l3 = snap.get('l3', {})
    crc_valid = l2.get('crc_valid', False)
    crc_txt = "VALID" if crc_valid else "GAGAL"

    # Lokasi QRIS (tag 60) bisa didapat dari L2; kalau L3 tak punya, fallback.
    qris_city = (l3.get('merchant_city') or l2.get('merchant_city') or None)
    client_city = l3.get('client_city')

    lines = [
        "===== SCAN %s ====" % datetime.now().strftime("%H:%M:%S"),
        "QR: %s" % (snap.get('raw_qris_str') or "(kosong)"),
        "Blur: var=%.1f %s" % (snap.get('blur_var', 0), "BLUR" if snap.get('is_blurry') else "CLEAR"),
        "L1 optik: edge=%.4f glare=%.5f skor=%.3f (%s)" % (
            l1.get('spatial_edge_density', 0), l1.get('temporal_glare_var', 0),
            l1.get('l1_score', 0), l1.get('risk_level', 'N/A')),
        "L2 EMVCo: TLV=%s CRC=%s (encoded=%s)" % (
            "VALID" if l2.get('parsed_tlv', {}).get('valid') else "INVALID",
            crc_txt,
            str(snap.get('raw_qris_str') or '')[-4:] or "?"),
        "L2: merchant=%s kota=%s mcc=%s mode=%s" % (
            l2.get('merchant_name') or "N/A", l2.get('merchant_city') or "N/A",
            l2.get('mcc') or "N/A", l2.get('initiation_mode') or "N/A"),
        "LOKASI QRIS (tag 60): %s" % (qris_city or "N/A"),
        "LOKASI USER (GPS): %s" % (client_city or "TIDAK ADA (izin/GPS mati)"),
        "L3 geofence: skor=%.2f status=%s" % (l3.get('l3_score', 0), l3.get('risk_level', "N/A")),
        "GABUNGAN: skor=%.3f risk=%s" % (snap.get('combined_score', 0), snap.get('combined_risk_level', 'N/A')),
        "Warnings: %s" % ("; ".join((l2.get('warnings') or []) + (l3.get('warnings') or [])) or "tidak ada"),
    ]
    detail = "\n".join(lines)
    log_crash("SCAN", detail)
    for line in lines:
        _log_to_adb("ANTITIMPA", line)
    return detail


def _install_crash_hooks():
    """Pasang sys.excepthook + threading.excepthook supaya setiap uncaught
    exception (di thread mana pun, termasuk thread analisis Camera4Kivy)
    ditulis ke log dan TIDAK diam-diam mematikan app.

    Catatan: sys.excepthook tidak mencegah Kivy menutup app kalau exception
    terjadi di dalam callback Kivy (itu memang fatal). Tapi kita catat dulu
    traceback-nya supaya bisa dikirim. Untuk error yang tidak fatal, app terus
    jalan dan kita tampilkan banner di UI.
    """
    sys_excepthook = sys.excepthook

    def _handler(exc_type, exc, exc_tb):
        try:
            log_crash(
                "UNCAUGHT EXCEPTION",
                "App akan ditutup oleh Kivy. Kirim log ini.",
                e=exc, tb=exc_tb, always_print=True,
            )
        except Exception:
            pass
        # Teruskan ke handler asli supaya perilaku default tetap.
        sys_excepthook(exc_type, exc, exc_tb)

    sys.excepthook = _handler

    try:
        import threading
        th_excepthook = threading.excepthook

        def _th_handler(args):
            try:
                log_crash(
                    "THREAD EXCEPTION [%s]" % args.name,
                    "Error di thread non-UI (kamera/analisis), tidak fatal.",
                    e=args.exc_value, tb=args.exc_traceback, always_print=True,
                )
            except Exception:
                pass
            th_excepthook(args)

        threading.excepthook = _th_handler
    except Exception:
        pass


_install_crash_hooks()

from kivy.clock import Clock, mainthread
from kivy.lang import Builder
from kivy.graphics import Color, Rectangle
from kivy.graphics.texture import Texture
from kivy.metrics import dp
from kivy.network.urlrequest import UrlRequest
from kivy.uix.image import Image as KivyImage
from kivymd.app import MDApp
from kivymd.uix.label import MDLabel

try:
    from plyer import gps as _plyer_gps
    gps = _plyer_gps
    if _plyer_gps is not None and not getattr(_plyer_gps, '_antitimpa_patched', False):
        # plyer versi lama tidak mendeklarasikan onLocationChanged(List) yang
        # dipanggil Android 15 (API 35) -> NotImplementedError -> lokasi tidak
        # pernah diterima. Di-patch di bawah (module-level) agar GPS jalan.
        pass
except Exception:
    gps = None


def _monkeypatch_android_gps():
    """Perbaiki GPS Android: plyer versi lama tidak mendukung onLocationChanged
    (List<Location>) yang dipanggil Android 15 (batch API), sehingga melempar
    NotImplementedError dan lokasi client tidak pernah diterima. Kita ganti
    listener + perilaku _start GPS dengan implementasi yang mendukung batch."""
    global gps
    if gps is None:
        return
    try:
        from plyer import gps as _g
        if getattr(_g, '_antitimpa_gps_patched', False):
            gps = _g
            return
        from jnius import java_method, PythonJavaClass, autoclass
        from plyer.platforms.android import activity, gps as _android_gps

        Looper = autoclass('android.os.Looper')
        Context = autoclass('android.content.Context')

        class _FixedListener(PythonJavaClass):
            __javainterfaces__ = ['android/location/LocationListener']

            def __init__(self, root):
                self.root = root
                super().__init__()

            def _emit(self, location):
                self.root.on_location(
                    lat=location.getLatitude(),
                    lon=location.getLongitude(),
                    speed=location.getSpeed(),
                    bearing=location.getBearing(),
                    altitude=location.getAltitude(),
                    accuracy=location.getAccuracy())

            @java_method('(Landroid/location/Location;)V', name='onLocationChanged')
            def onLocationChangedSingle(self, location):
                self._emit(location)

            @java_method('(Ljava/util/List;)V', name='onLocationChanged')
            def onLocationChangedBatch(self, location_list):
                # Batch API (Android 15): ambil lokasi terakhir dari daftar.
                try:
                    location = location_list.get(location_list.size() - 1)
                    self._emit(location)
                except Exception:
                    pass

            @java_method('(Ljava/lang/String;)V')
            def onProviderEnabled(self, status):
                if self.root.on_status:
                    self.root.on_status('provider-enabled', status)

            @java_method('(Ljava/lang/String;)V')
            def onProviderDisabled(self, status):
                if self.root.on_status:
                    self.root.on_status('provider-disabled', status)

            @java_method('(Ljava/lang/String;ILandroid/os/Bundle;)V')
            def onStatusChanged(self, provider, status, extras):
                if self.root.on_status:
                    self.root.on_status('provider-status', '{}: {}'.format(
                        provider, status))

        def _fixed_start(self, **kwargs):
            min_time = kwargs.get('minTime')
            min_distance = kwargs.get('minDistance')
            if not hasattr(self, '_fixed_location_manager'):
                self._fixed_location_manager = activity.getSystemService(
                    Context.LOCATION_SERVICE)
                self._fixed_listener = _FixedListener(self)
            lm = self._fixed_location_manager
            for provider in lm.getProviders(False).toArray():
                try:
                    lm.requestLocationUpdates(
                        provider, min_time, min_distance, self._fixed_listener,
                        Looper.getMainLooper())
                except Exception:
                    pass

        _android_gps.AndroidGPS._start = _fixed_start
        _g._antitimpa_gps_patched = True
        gps = _g
    except Exception as _e:
        try:
            log_crash("GPS PATCH ERROR", "Gagal patch plyer gps.", e=_e)
        except Exception:
            pass


_monkeypatch_android_gps()

# Layer 2 is always pure-Python (stdlib only).
from layer2_emvco import process_layer2_tlv

# --------------------------------------------------------------------------
# Try to load the full dual-layer pipeline. It requires numpy + cv2.
# KESALAHAN DI SINI DICATAT ke log, karena ini penyebab paling umum auto-close
# (modul tak ter-paket ke APK / import OpenCV gagal). Tapi kami TIDAK melempar
# exception — kami jatuh ke mode Layer-2-only supaya app tetap jalan.
# --------------------------------------------------------------------------
try:
    import cv2
    import numpy as np
    from scanner_core import QrisScannerCore
    from layer1_optical import expand_bounding_box
    import layer2_emvco  # noqa: F401 (ensure module packaged)

    DUAL_LAYER = True
    log_crash("INFO", "Dual-layer pipeline TERHUAT (cv2+numpy tersedia).")
except Exception as _e:
    log_crash(
        "IMPORT ERROR (dua-layer tidak aktif)",
        "Gagal import cv2/numpy/scanner_core/layer1_optical. App dilanjutkan "
        "ke mode Layer-2-only. Penyebab paling umum: modul tidak ter-paket "
        "ke APK, atau OpenCV gagal dimuat.",
        e=_e, tb=_e.__traceback__, always_print=True,
    )
    cv2 = None
    np = None
    DUAL_LAYER = False

# --------------------------------------------------------------------------
# Camera4Kivy (bridge kamera nyata). Saat tersedia, kita pakai Preview-nya
# untuk menganalisis frame camera langsung. Kalau tidak, fallback sintetik.
# --------------------------------------------------------------------------
try:
    from camera4kivy import Preview as C4KPreview
    CAMERA4KIVY = True
except Exception as _e:  # pragma: no cover - camera4kivy absent in light build
    log_crash("INFO", "camera4kivy tidak tersedia — pakai feed sintetik.",
              e=_e)
    C4KPreview = None
    CAMERA4KIVY = False

# --------------------------------------------------------------------------
# Tiga KV lengkap (bukan template + format). Setiap preview widget ditulis
# langsung dengan indentasi yang benar (di dalam MDBoxLayout indent 8,
# children indent 12), sehingga tidak ada masalah indent saat di-load.
# --------------------------------------------------------------------------
_KV_REAL = '''MDScreen:
    md_bg_color: 0.07, 0.08, 0.10, 1

    MDTopAppBar:
        id: topbar
        title: "Anti Timpa QRIS (L1 + L2)"
        md_bg_color: 0.12, 0.31, 0.71, 1

    MDBoxLayout:
        id: root_box
        orientation: "vertical"
        padding: "12dp"
        spacing: "8dp"

        MDLabel:
            id: log_label
            text: ""
            theme_text_color: "Custom"
            text_color: 1.0, 0.6, 0.3, 1
            font_style: "Caption"
            size_hint_y: None
            height: 0
            opacity: 0

        MDLabel:
            id: mode_label
            text: ""
            bold: True
            theme_text_color: "Custom"
            text_color: 0.6, 0.9, 0.9, 1
            size_hint_y: None
            height: "28dp"

        CameraLivePreview:
            id: preview
            size_hint_y: None
            height: "240dp"
            aspect_ratio: '4:3'
            letterbox_color: 0.10, 0.11, 0.14, 1

        MDTextField:
            id: payload_input
            hint_text: "Tempel payload QRIS (opsional di mode Layer 1)"
            multiline: False

        MDBoxLayout:
            orientation: "horizontal"
            adaptive_height: True
            spacing: "8dp"
            size_hint_y: None
            height: "52dp"
            MDRaisedButton:
                id: run_btn
                text: "Ambil Foto QR (One-Shot)"
                on_release: app.start_or_analyze()
            MDRaisedButton:
                text: "Import Gambar"
                theme_text_color: "Custom"
                text_color: 1, 1, 1, 1
                on_release: app.import_image()
            MDRaisedButton:
                text: "Stop"
                theme_text_color: "Custom"
                text_color: 1, 0.6, 0.6, 1
                on_release: app.stop_work()

        MDLabel:
            id: result_title
            text: "Belum ada analisis."
            bold: True
            theme_text_color: "Custom"
            text_color: 1, 1, 1, 1
            font_style: "H6"
            size_hint_y: None
            height: "40dp"

        MDLabel:
            id: log_detail
            text: "Log detail scan akan tampil di sini (dan dikirim ke adb logcat)."
            theme_text_color: "Custom"
            text_color: 0.7, 0.95, 0.7, 1
            font_style: "Caption"
            size_hint_y: None
            height: "140dp"
            text_size: self.width - 12, None
            halign: "left"
            valign: "top"

        ScrollView:
            MDBoxLayout:
                id: result_box
                orientation: "vertical"
                adaptive_height: True
                spacing: "6dp"
'''

_SYN_HEAD = _KV_REAL.split("CameraLivePreview:")[0]
_SYN_MID = "KivyImage:\n            id: preview\n            size_hint_y: None\n            height: \"240dp\"\n            bg_color: 0.10, 0.11, 0.14, 1\n            allow_stretch: True\n            keep_ratio: True\n"
_SYN_TAIL = _KV_REAL.split("aspect_ratio: '4:3'")[1]
_KV_SYN = _SYN_HEAD + _SYN_MID + _SYN_TAIL

_NONE_MID = "Widget:\n            id: preview\n            size_hint_y: None\n            height: 0\n"
_KV_NONE = _SYN_HEAD + _NONE_MID + _SYN_TAIL

KV = _KV_REAL  # default (akan dipilih di build())


# --------------------------------------------------------------------------
# Widget kamera nyata (subclass dari Camera4Kivy Preview)
# --------------------------------------------------------------------------
if CAMERA4KIVY:
    class CameraLivePreview(C4KPreview):
        """Analyze real camera frames through the dual-layer engine.

        `analyze_pixels_callback()` berjalan di thread analisis (non-UI). Kita
        simpan hasil ke texture RGBA secara thread-safe, lalu gambar di
        `canvas_instructions_callback()` (thread UI).
        """

        def __init__(self, app_ref=None, **kwargs):
            super().__init__(**kwargs)
            self._app = app_ref            # AntiTimpaMobileApp (di-set ulang di build())
            self._frame_texture = None     # Texture RGBA -> ditampilkan
            self._frame_rect = None
            self._skip = 0                 # throttle preview (hemet CPU)
            self._last_pixels = None       # buffer RGBA frame terakhir (utk tombol)
            self._last_size = None         # (w, h) frame terakhir

        # -- Kamera: preview-only (SANGAT ringan) -------------------------------
        # Thread kamera HANYA menampilkan preview. TIDAK ada cvtColor/detect_qr
        # per frame di sini — itu yang bikin berat sampai hang di device rendah.
        # Capture + analisis dilakukan EKSPLISIT saat pengguna menekan tombol
        # "Ambil Foto QR" (lihat app.take_photo()), lalu decode di thread yang
        # sama dengan preview di-render tanpa konversi ekstra.
        def analyze_pixels_callback(self, pixels, image_size, image_pos,
                                    image_scale, mirror):
            app = self._app
            if app is None or not app.dual:
                return
            self._last_pixels = pixels          # simpan buffer terbaru utk tombol
            self._last_size = image_size
            # Preview di-update tiap 2 frame (untuk device rendah), buffer tiap
            # frame tetap disimpan untuk tombol. Tanpa cvtColor/detect per frame.
            self._skip += 1
            if self._skip % 2 != 0:
                return
            self._set_texture(pixels, image_size[0], image_size[1])

        @mainthread
        def _set_texture(self, rgba_bytes, w, h):
            # Pakai ulang texture bila ukurannya sama (hemat alokasi; blit ulang
            # murah). Buat baru hanya bila ukuran berubah.
            if (self._frame_texture is None
                    or (self._frame_texture.width, self._frame_texture.height) != (w, h)):
                tex = Texture.create(size=(w, h), colorfmt="rgba")
                self._frame_texture = tex
            tex = self._frame_texture
            tex.blit_buffer(rgba_bytes, colorfmt="rgba", bufferfmt="ubyte")
            tex.flip_vertical()

        # -- Tampilkan hasil analisis (still frame + HUD) ---------------------
        @mainthread
        def show_frame_oneshot(self, frame_bgr):
            """Tampilkan frame yang dipakai menganalisis + HUD, sebagai still
            (preview foto one-shot)."""
            try:
                h, w = frame_bgr.shape[:2]
                rgba = cv2.cvtColor(frame_bgr, cv2.COLOR_BGR2RGBA)
                self._set_texture(rgba.tobytes(), w, h)
            except Exception:
                pass

        # -- UI-thread rendering ---------------------------------------------
        def canvas_instructions_callback(self, texture, tex_size, tex_pos):
            if self._frame_texture is None:
                return
            self.canvas.after.clear()
            with self.canvas.after:
                Color(1, 1, 1, 1)
                self._frame_rect = Rectangle(texture=self._frame_texture,
                                             pos=tex_pos, size=tex_size)

        def connect(self):
            # resolusi analisis diturunkan + tidak butuh video: lebih ringan.
            self.connect_camera(enable_analyze_pixels=True,
                                enable_video=False,
                                analyze_pixels_resolution=480)

        def disconnect(self):
            self.disconnect_camera()
else:
    # camera4kivy tidak tersedia -- takdir SyntheticPreview dipakai.
    class CameraLivePreview(object):
        _app = None

        def __init__(self, *a, **k):
            raise RuntimeError("camera4kivy tidak tersedia di build ini")

        def connect(self):
            raise RuntimeError("camera4kivy tidak tersedia di build ini")

        def disconnect(self):
            pass


# --------------------------------------------------------------------------
# Preview sintetik (Image sederhana) -- fallback bila kamera tidak tersedia.
# --------------------------------------------------------------------------
class SyntheticPreview(KivyImage):
    def show_frame(self, frame_bgr):
        if frame_bgr is None:
            return
        h, w = frame_bgr.shape[:2]
        buf = np.flip(frame_bgr, axis=0).tobytes()
        texture = Texture.create(size=(w, h), colorfmt="bgr")
        texture.blit_buffer(buf, colorfmt="bgr", bufferfmt="ubyte")
        texture.flip_vertical()
        self.texture = texture


class AntiTimpaMobileApp(MDApp):

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.dual = DUAL_LAYER
        self.camera_ok = DUAL_LAYER and CAMERA4KIVY
        self.core = None                     # QrisScannerCore (dual-layer)
        self.is_running = False
        self.sim_count = 0
        self.notify_ev = None
        self._preview_widget = None          # CameraLivePreview atau SyntheticPreview
        self._snapshot = None                # snapshot terakhir (thread-safe)
        self._snap_scheduled = False         # flag 'latest-only' update hasil

        self.client_city = None
        self.last_gps_coords = None
        self.is_fetching_city = False
        self._captured = False          # one-shot: sudah ada hasil tangkapan?
        self._processing = False        # one-shot: analisis sedang berjalan?
        self._last_pixels = None        # frame kamera RGBA terakhir (utk tombol)
        self._last_size = None          # (w, h) frame terakhir

        if self.dual:
            self.core = QrisScannerCore(blur_threshold=100.0, fifo_size=5)

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------
    def build(self):
        self.theme_cls.primary_palette = "Blue"
        self.theme_cls.theme_style = "Dark"

        # Pilih KV penuh sesuai mode (tidak pakai .format() agar indent aman).
        if self.dual and self.camera_ok:
            kv = _KV_REAL
            mode = "Kamera NYATA (Camera4Kivy) — Layer 1 optik AKTIF"
        elif self.dual:
            kv = _KV_SYN
            mode = "Modus SINTETIK (demo) — Layer 1 optik AKTIF"
        else:
            kv = _KV_NONE
            mode = "Layer 2 saja  |  OpenCV tidak tersedia di build ini"

        root = Builder.load_string(kv)
        self.root = root
        self.root.ids.mode_label.text = "Mode: " + mode

        # Tampilkan path log (supaya jelas ke mana mengirim file saat nge-report).
        try:
            self.root.ids.mode_label.text += ("  [Log: %s]" % self.log_path())
        except Exception:
            pass

        if self.dual and self.camera_ok:
            self.root.ids.topbar.title = "Anti Timpa QRIS (L1 + L2)"
            pw = self.root.ids.get("preview")
            if isinstance(pw, CameraLivePreview):
                pw._app = self
                self._preview_widget = pw
        elif self.dual:
            self.root.ids.topbar.title = "Anti Timpa QRIS (L1 + L2)"
        else:
            self.root.ids.topbar.title = "Anti Timpa QRIS (Layer 2)"
        return self.root

    def _start_camera(self):
        if isinstance(self._preview_widget, CameraLivePreview):
            try:
                self._preview_widget.connect()
                self._set_status("Kamera aktif (Lokal). Arahkan ke QRIS.")
            except Exception as e:
                log_crash("CAMERA CONNECT ERROR", "Gagal membuka kamera.", e=e,
                          tb=e.__traceback__, always_print=True)
                self._set_status(f"Gagal buka kamera: {e}")
                self.show_log_in_ui("Kamera gagal: %s" % e)
        self.is_running = True

    def on_start(self):
        # Tampilkan isi log error terakhir di UI supaya mudah dikirim.
        self._show_latest_log()
        # Camera4Kivy mengharuskan izin kamera diminta TERLEBIH DAHULU (setelah
        # on_start), lalu sambungkan kamera. Jangan tautkan di build().
        if self.camera_ok:
            try:
                from android.permissions import (
                    request_permissions, Permission, check_permission)
                if not check_permission(Permission.CAMERA):
                    request_permissions(
                        [Permission.CAMERA, Permission.ACCESS_FINE_LOCATION, Permission.ACCESS_COARSE_LOCATION],
                        self._granted)
                else:
                    self._granted([])
                return
            except Exception as _e:
                log_crash("PERMISSION API ERROR",
                          "android.permissions tidak tersedia; lanjut mencoba kamera.",
                          e=_e)
        # Tanpa platform android (desktop) atau tanpa permission api.
        Clock.schedule_once(lambda dt: self._start_camera(), 0.0)

    def _granted(self, permissions):
        # Apapun hasil izin, coba tautkan kamera; Camera4Kivy menangani
        # penolakan dengan pesan sendiri.
        self._start_gps()
        Clock.schedule_once(lambda dt: self._start_camera(), 0.0)

    def _start_gps(self):
        # Layer 3 GPS: ambil koordinat lalu reverse-geocode ke nama kota.
        if gps:
            try:
                gps.configure(on_location=self._on_location)
                gps.start(minTime=10000, minDistance=50)  # every 10s or 50m
            except Exception as e:
                log_crash("GPS START ERROR", "Gagal start GPS.", e=e)
        # Ambil lokasi terakhir yang dikenal (instant, tanpa menunggu fix GPS
        # satelit). Ini membuat L3 cepat dapat kota walau di dalam ruangan.
        self._try_last_known_location()

    def _try_last_known_location(self):
        """Ambil last-known location dari LocationManager (fused/network), lalu
        masukkan ke alur yang sama dengan fix GPS. Berguna saat di dalam ruangan
        dan GPS satelit belum dapat fix baru."""
        if self.client_city and self.client_city != "LOADING":
            return
        try:
            from jnius import autoclass
            from plyer.platforms.android import activity
            Looper = autoclass('android.os.Looper')
            LocationManager = autoclass('android.location.LocationManager')
            Context = autoclass('android.content.Context')
            lm = activity.getSystemService(Context.LOCATION_SERVICE)
            best = None
            for provider in lm.getProviders(False).toArray():
                try:
                    loc = lm.getLastKnownLocation(provider)
                    if loc is not None and (best is None or loc.getTime() > best.getTime()):
                        best = loc
                except Exception:
                    pass
            if best is not None:
                lat = best.getLatitude()
                lon = best.getLongitude()
                _log_to_adb("ANTITIMPA", "LAST KNOWN LOC: lat=%s lon=%s" % (lat, lon))
                self.last_gps_coords = (lat, lon)
                # Panggil langsung (bukan lewat @mainthread) supaya pasti jalan.
                self._fetch_city_from_coords(lat, lon)
        except Exception as _e:
            log_crash("LASTKNOWNNLOC ERROR", "Gagal ambil last-known location.", e=_e)


    @mainthread
    def _on_location(self, **kwargs):
        lat = kwargs.get('lat')
        lon = kwargs.get('lon')
        if lat and lon:
            self.last_gps_coords = (lat, lon)
            self._fetch_city_from_coords(lat, lon)

    def _fetch_city_from_coords(self, lat, lon):
        if self.is_fetching_city:
            return
        self.is_fetching_city = True
        if self.client_city is None:
            self.client_city = "LOADING"
            _log_to_adb("ANTITIMPA", "GEOCODE: meminta kota utk %.5f,%.5f" % (lat, lon))

        url = f"https://nominatim.openstreetmap.org/reverse?lat={lat}&lon={lon}&format=json&zoom=10"
        try:
            UrlRequest(
                url,
                on_success=self._on_city_success,
                on_failure=self._on_city_fail,
                on_error=self._on_city_fail,
                req_headers={'User-Agent': 'AntiTimpaApp/0.1.0'}
            )
        except Exception as _e:
            log_crash("GEOCODE ERROR", "Gagal reverse geocoding.", e=_e)
            self.is_fetching_city = False

    def _on_city_success(self, req, result):
        self.is_fetching_city = False
        try:
            address = result.get("address", {})
            city = address.get("city") or address.get("town") or address.get("county")
            if city:
                self.client_city = city.upper()
                _log_to_adb("ANTITIMPA", "GEOCODE OK: kota=%s" % self.client_city)
            else:
                _log_to_adb("ANTITIMPA", "GEOCODE OK tapi kota kosong: address=%s" % address)
        except Exception as e:
            _log_to_adb("ANTITIMPA", "GEOCODE PARSE ERROR: %s" % str(e))

    def _on_city_fail(self, req, error):
        self.is_fetching_city = False
        _log_to_adb("ANTITIMPA", "GEOCODE FAIL: %s" % str(error))

    def _set_status(self, msg):
        mode = self.root.ids.mode_label if self.root else None
        if mode is not None:
            mode.text = msg

    # ------------------------------------------------------------------
    # Tampilkan pesan log/error di UI (bukan cuma ditelan), sekaligus tulis
    # ke file log agar bisa dikirim via adb.
    # ------------------------------------------------------------------
    def show_log_in_ui(self, text):
        """Tampilkan baris status log di banner atas layar."""
        try:
            lbl = self.root.ids.log_label if self.root else None
        except Exception:
            lbl = None
        if lbl is None:
            return
        if text:
            lbl.text = str(text)[:300]
            lbl.opacity = 1
            lbl.height = dp(48)
        else:
            lbl.text = ""
            lbl.opacity = 0
            lbl.height = 0

    def _notify(self, msg):
        """Tampilkan pesan ke pengguna. Tidak memakai Snackbar karena di
        KivyMD 1.2.0 `Snackbar(text=...)` melempar error (properti `text`
        tidak sah). Dipakai banner status yang sudah terbukti stabil."""
        self.show_log_in_ui(msg)
        log_crash("NOTIFY", str(msg))

    def _show_latest_log(self):
        """Baca baris-baris penting dari file log (semua yang bukan INFO biasa)
        dan tampilkan di UI, supaya error yang tersembunyi terlihat."""
        try:
            path = self.log_path()
            if not path or not os.path.exists(path):
                return
            with open(path, "r") as f:
                lines = f.read().splitlines()
            # ambil blok yang mengandung kata kunci error
            important = [l for l in lines
                         if any(k in l.upper() for k in
                                ("ERROR", "EXCEPTION", "GAGAL", "FAIL",
                                 "TIDAK TERSEDIA", "IMPORT"))]
            if important:
                self.show_log_in_ui("Error terdeteksi saat startup:\n" +
                                    "\n".join(important[-6:]))
        except Exception:
            pass

    @staticmethod
    def log_path():
        """Path file log agar pengguna tahu ke mana harus mengirimkan."""
        return LOG_PATH or _log_path()

    # ------------------------------------------------------------------
    # Payload override dari kotak input
    # ------------------------------------------------------------------
    def pasted_raw(self):
        try:
            return self.root.ids.payload_input.text.strip() or None
        except Exception:
            return None

    def post_snapshot(self, snap):
        """Catat snapshot terbaru lalu jadwalkan render ke main thread sekali,
        mengganti jadwal sebelumnya ('latest-only'). Ini mencegah antrian frame
        menumpuk dan membocorkan memori saat main thread sibuk."""
        self._snapshot = snap
        if not getattr(self, "_snap_scheduled", False):
            self._snap_scheduled = True
            Clock.schedule_once(self._render_latest, 0)

    def _render_latest(self, _dt):
        self._snap_scheduled = False
        if self._snapshot is not None:
            try:
                self._render_snapshot(self._snapshot)
            except Exception:
                pass

    # ------------------------------------------------------------------
    # Entry action
    # ------------------------------------------------------------------
    def start_or_analyze(self):
        if not self.dual:
            self.analyze_text_only()
            return
        if self.camera_ok:
            # Tombol "Ambil Foto QR": ambil SATU frame lalu analisis.
            self.take_photo()
            return
        if not self.is_running:
            self.start_synthetic()
        else:
            self._tick(0)

    def take_photo(self):
        """One-shot: ambil frame kamera terakhir lalu analisis (seperti Import
        Gambar, bedanya sumbernya kamera). Log rincian dikirim ke UI + adb
        ANTITIMPA saat mulai dan saat selesai."""
        if self._processing or self._captured:
            _log_to_adb("ANTITIMPA", "take_photo: dilewati (processing/captured)")
            self._notify("Sedang/sudah ada hasil. Ketuk lagi setelah siap.")
            return

        pw = self._preview_widget
        if pw is None or getattr(pw, "_last_pixels", None) is None:
            _log_to_adb("ANTITIMPA", "take_photo: belum ada frame kamera")
            self._notify("Kamera belum mengirim frame.")
            return

        # Kalau ada payload tempelan, pakai itu saja (tanpa decode gambar).
        pasted = self.pasted_raw()
        if pasted:
            frame_bgr = self._pixels_to_bgr()
            if frame_bgr is None:
                self._notify("Frame kamera tidak valid.")
                return
            snap = self.core.process_frame(
                frame_bgr, optical_type="physical_camera_scan",
                force_raw=pasted, client_city=self.client_city)
            self._present_result(snap, frame_bgr)
            return

        self._processing = True
        _log_to_adb("ANTITIMPA", "TAKE PHOTO START")
        self._notify("Mengambil & menganalisis foto...")

        def _work():
            try:
                frame_bgr = self._pixels_to_bgr()
                if frame_bgr is None:
                    _log_to_adb("ANTITIMPA", "TAKE PHOTO ERROR: frame invalid")
                    return
                h_img, w_img = frame_bgr.shape[:2]
                _log_to_adb("ANTITIMPA", "frame: %dx%d" % (w_img, h_img))

                result = QrisScannerCore.analyze_image(
                    frame_bgr, optical_type="physical_camera_scan",
                    client_city=self.client_city)
                _log_to_adb("ANTITIMPA", "analyze_image ok=%s raww=%s" % (
                    result.get('ok'), bool(result.get('raw'))))
                if result.get('ok') and result.get('raw'):
                    self._present_result(result['snapshot'], frame_bgr)
                else:
                    _log_to_adb("ANTITIMPA", "NO QR: " + str(
                        result.get('error', 'tidak ada payload ter-decode')))
                    self._notify("Tidak ada QR terbaca pada foto. Coba lagi.")
            except Exception as _e:
                log_crash("TAKE PHOTO ERROR", str(_e), e=_e,
                          tb=_e.__traceback__, always_print=True)
                _log_to_adb("ANTITIMPA", "TAKE PHOTO ERROR: %s" % str(_e))
            finally:
                self._processing = False

        threading.Thread(target=_work, daemon=True).start()

    def _pixels_to_bgr(self):
        """Ubah frame RGBA terakhir dari kamera menjadi numpy BGR, dengan
        perkecilan bila perlu. Kembalikan None bila tidak valid."""
        pw = self._preview_widget
        px = getattr(pw, "_last_pixels", None)
        size = getattr(pw, "_last_size", None)
        if not isinstance(px, (bytes, bytearray, memoryview)) or not size:
            return None
        w, h = size
        try:
            rgba = np.frombuffer(px, dtype=np.uint8).reshape((h, w, 4))
            bgr = cv2.cvtColor(rgba, cv2.COLOR_RGBA2BGR)
            return bgr
        except Exception:
            return None

    def _present_result(self, snap, frame_bgr):
        """Kirim snapshot ke UI + log detail ke adb, dan tunjukkan HUD pada
        frame yang dipakai menganalisis."""
        self._captured = True
        self._snapshot = snap
        self.post_snapshot(snap)
        try:
            pw = self._preview_widget
            if frame_bgr is not None and pw is not None and hasattr(pw, 'show_frame_oneshot'):
                pw.show_frame_oneshot(self._draw_hud(frame_bgr, snap))
        except Exception:
            pass
        self._notify("Analisis foto selesai (log detail di layar & adb).")

    def analyze_text_only(self):
        raw = self.root.ids.payload_input.text.strip()
        if not raw:
            self._notify("Masukkan payload QRIS terlebih dahulu.")
            return
        result = process_layer2_tlv(raw, scan_context={"optical_type": "physical_camera_scan"})
        self._render_text_only(result)

    # ------------------------------------------------------------------
    # Dual-layer synthetic feed (fallback, mencerminkan app/main.py)
    # ------------------------------------------------------------------
    def start_synthetic(self):
        if self.is_running:
            return
        self.is_running = True
        self.sim_count = 0
        self.notify_ev = Clock.schedule_interval(self._tick, 1.0 / 30.0)
        self._set_status("Analisis berjalan (Lokal) — Layer 1 optik AKTIF.")

    def stop_work(self):
        self.is_running = False
        self._captured = False
        self._processing = False
        if self.notify_ev:
            self.notify_ev.cancel()
            self.notify_ev = None
        if isinstance(self._preview_widget, CameraLivePreview):
            try:
                self._preview_widget.disconnect()
            except Exception as _e:
                log_crash("DISCONNECT ERROR", "Gagal memutus kamera.", e=_e)
        self._set_status("Dihentikan. Ketuk 'Analisis' untuk mulai lagi.")

    def _synthetic_frame(self):
        frame = np.full((480, 640, 3), 235, dtype=np.uint8)
        qx, qy, qw, qh = 200, 140, 240, 240
        cv2.rectangle(frame, (qx - 20, qy - 20), (qx + qw + 20, qy + qh + 20), (245, 245, 245), -1)
        cv2.rectangle(frame, (qx, qy), (qx + qw, qy + qh), (0, 0, 0), 4)
        cv2.rectangle(frame, (qx + 10, qy + 10), (qx + 60, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + qw - 60, qy + 10), (qx + qw - 10, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + 10, qy + qh - 60), (qx + 60, qy + qh - 10), (0, 0, 0), -1)

        # Anomali stiker fisik periodik -> memicu Layer 1 edge density.
        if (self.sim_count // 60) % 2 == 1:
            cv2.rectangle(frame, (qx - 15, qy - 15), (qx + qw + 15, qy + qh + 15), (30, 30, 30), 4)
            cv2.rectangle(frame, (qx - 10, qy - 10), (qx + qw + 10, qy + qh + 10), (170, 170, 170), 2)

        # Glare specular periodik -> memicu glare variance.
        if self.sim_count % 90 < 15:
            cv2.circle(frame, (qx + 120, qy + 120), 8, (255, 255, 255), -1)
        return frame

    def _tick_synthetic_payload(self):
        mode = (self.sim_count // 60) % 3
        if mode == 0:
            return "00020101021126330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG MAKMUR6007JAKARTA63041B52"
        elif mode == 1:
            return "00020101021126330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG HACKED6007JAKARTA63041B52"
        else:
            return "00020101021226330010A0000006020115ID10200000000015204541153033605802ID5919TOKO CHARITY BERKAH6007JAKARTA6304278F"

    def _tick(self, _dt):
        if not self.is_running:
            return
        self.sim_count += 1
        frame = self._synthetic_frame()
        force_bbox = (200, 140, 240, 240)
        force_raw = self._tick_synthetic_payload()
        pasted = self.root.ids.payload_input.text.strip()
        if pasted:
            force_raw = pasted

        snap = self.core.process_frame(
            frame, optical_type="physical_camera_scan",
            force_bbox=force_bbox, force_raw=force_raw,
            client_city=self.client_city,
        )
        hud = self._draw_hud(frame, snap)
        self._show_preview(hud)
        self._render_snapshot(snap)

    # ------------------------------------------------------------------
    # HUD + rendering
    # ------------------------------------------------------------------
    def _draw_hud(self, frame, snap):
        display = frame.copy()
        if snap['qr_bbox'] is not None and not snap['is_blurry']:
            x, y, bw, bh = snap['qr_bbox']
            box_color = self._risk_color(snap['combined_score'], snap['l2']['crc_valid'])
            cv2.rectangle(display, (x, y), (x + bw, y + bh), box_color, 3)
            try:
                x_exp, y_exp, w_exp, h_exp = expand_bounding_box(
                    snap['qr_bbox'], display.shape[1], display.shape[0], padding_percent=0.10)
                cv2.rectangle(display, (x_exp, y_exp), (x_exp + w_exp, y_exp + h_exp), box_color, 1)
            except Exception as _e:
                log_crash("HUD ERROR", "Gagal menggambar bbox luar.", e=_e)
        return display

    @staticmethod
    def _risk_color(score, crc_valid):
        if score < 0.35 and crc_valid:
            return (0, 255, 0)
        elif score <= 0.70 and crc_valid:
            return (0, 255, 255)
        return (0, 0, 255)

    def _show_preview(self, frame_bgr):
        preview = self.root.ids.get("preview")
        if isinstance(preview, SyntheticPreview):
            preview.show_frame(frame_bgr)

    def _apply_risk_style(self, title_lbl, score, crc_valid, risk_level=None):
        risk_level = risk_level or "HIGH RISK (CRC gagal)" if not crc_valid else ("LOW RISK" if score < 0.35 else ("CAUTION" if score <= 0.70 else "HIGH RISK"))
        if risk_level in ("MENUNGGU SCAN", "SCANNING", "NO QR"):
            title_lbl.text = f"{risk_level}  |  Skor: {score:.3f}"
            title_lbl.text_color = [0.6, 0.9, 0.9, 1]
        elif not crc_valid:
            title_lbl.text = f"HIGH RISK (CRC gagal)  |  Skor: {score:.3f}"
            title_lbl.text_color = [1, 0.4, 0.4, 1]
        elif score < 0.35:
            title_lbl.text = f"LOW RISK  |  Skor: {score:.3f}"
            title_lbl.text_color = [0.4, 1, 0.4, 1]
        elif score <= 0.70:
            title_lbl.text = f"CAUTION  |  Skor: {score:.3f}"
            title_lbl.text_color = [1, 1, 0.4, 1]
        else:
            title_lbl.text = f"HIGH RISK  |  Skor: {score:.3f}"
            title_lbl.text_color = [1, 0.4, 0.4, 1]

    def _clear_box(self):
        self.root.ids.result_box.clear_widgets()

    def _add_row(self, key, value):
        self.root.ids.result_box.add_widget(
            MDLabel(text=f"{key}: {value}", theme_text_color="Custom",
                    text_color=[0.85, 0.88, 0.92, 1], font_style="Caption",
                    size_hint_y=None, height=dp(22))
        )

    def _add_warnings(self, warnings):
        box = self.root.ids.result_box
        box.add_widget(MDLabel(text="Warnings:", bold=True, theme_text_color="Custom",
                               text_color=[1, 0.8, 0.6, 1],
                               size_hint_y=None, height=dp(22)))
        if warnings:
            for w in warnings:
                box.add_widget(MDLabel(text="• " + w, theme_text_color="Custom",
                                       text_color=[1, 0.65, 0.35, 1],
                                       font_style="Caption", size_hint_y=None, height=dp(22)))
        else:
            box.add_widget(MDLabel(text="• Tidak ada", theme_text_color="Custom",
                                   text_color=[0.4, 1, 0.4, 1],
                                   size_hint_y=None, height=dp(22)))

    def _render_snapshot(self, snap):
        title = self.root.ids.result_title
        l1 = snap['l1']
        l2 = snap['l2']
        l3 = snap.get('l3', {})
        self._apply_risk_style(title, snap['combined_score'], l2.get('crc_valid', True), snap.get('combined_risk_level'))
        self._clear_box()

        # Kirim log detail scan ke file + adb logcat, lalu tampilkan di UI.
        try:
            detail = log_scan_detail(snap)
            lbl = self.root.ids.get("log_detail")
            if lbl is not None:
                lbl.text = detail
        except Exception:
            pass

        self._add_row("Layer 1 - Edge Density (margin)", f"{l1.get('spatial_edge_density', 0):.4f}")
        self._add_row("Layer 1 - Glare Variance", f"{l1.get('temporal_glare_var', 0):.5f}")
        self._add_row("Layer 1 - Skor Optik", f"{l1.get('l1_score', 0):.3f}")
        self._add_row("Layer 1 - Status", l1.get('risk_level', 'N/A'))
        self._add_row("Layer 2 - Struktur TLV",
                      "VALID" if l2.get('parsed_tlv', {}).get('valid') else "INVALID")
        self._add_row("Layer 2 - CRC-16", "VALID" if l2.get('crc_valid', False) else "GAGAL")
        self._add_row("Layer 2 - Merchant", l2.get('merchant_name') or 'N/A')
        self._add_row("Layer 2 - Kota", l2.get('merchant_city') or 'N/A')
        self._add_row("Layer 2 - MCC", l2.get('mcc') or 'N/A')
        self._add_row("Layer 2 - Mode Inisiasi", l2.get('initiation_mode') or 'N/A')

        c_city = l3.get('client_city', 'N/A')
        if c_city in (None, '', 'N/A'):
            c_city = 'N/A (izin/GPS belum aktif)'
        self._add_row("Layer 3 - Lokasi QRIS", (l3.get('merchant_city') or l2.get('merchant_city') or 'N/A'))
        self._add_row("Layer 3 - Lokasi User", c_city)
        self._add_row("Layer 3 - Geofence Status", l3.get('risk_level', 'N/A'))
        self._add_row("Layer 3 - Skor", f"{l3.get('l3_score', 0.0):.3f}")

        all_warnings = l2.get('warnings', []) + l3.get('warnings', [])
        self._add_warnings(all_warnings)

    def _render_text_only(self, result):
        title = self.root.ids.result_title
        self._apply_risk_style(
            title, result.get("l2_score", 0.0), result.get("crc_valid", False))
        self._clear_box()

        self._add_row("Struktur TLV", "VALID" if result.get("parsed_tlv", {}).get("valid") else "INVALID")
        self._add_row("CRC-16", "VALID" if result.get("crc_valid", False) else "GAGAL")
        self._add_row("Merchant", result.get("merchant_name") or "N/A")
        self._add_row("Kota", result.get("merchant_city") or "N/A")
        self._add_row("MCC", result.get("mcc") or "N/A")
        self._add_row("Mode Inisiasi", result.get("initiation_mode") or "N/A")
        self._add_warnings(result.get("warnings", []))

    # ------------------------------------------------------------------
    # Import image (one-shot dual-layer validation, fully local)
    # ------------------------------------------------------------------
    def import_image(self):
        try:
            from plyer import filechooser
        except Exception as _e:
            log_crash("PLYER ERROR", "filechooser tidak tersedia.", e=_e)
            self._notify("File picker tidak tersedia di platform ini.")
            return
        if self.dual:
            self._notify("Pilih file QRIS dari galeri (lokal).")
            filechooser.open_file(on_selection=self._on_file_selected)
        else:
            self._notify("Import gambar butuh OpenCV (mode Layer 1).")

    def _on_file_selected(self, selection):
        if selection:
            self._analyze_imported_file(selection[0])

    def _analyze_imported_file(self, path):
        try:
            frame = cv2.imread(path)
            if frame is None:
                self._notify("Gagal membaca gambar.")
                return
            result = QrisScannerCore.analyze_image(frame, optical_type="imported_image")
            if not result['ok']:
                self._notify(result.get('error', "Tidak ada QR terdeteksi."))
                return
            self._render_snapshot(result['snapshot'])
            if isinstance(self.root.ids.get("preview"), SyntheticPreview):
                self._show_preview(self._draw_hud(frame, result['snapshot']))
            self._set_status("Analisis gambar selesai (lokal, Layer 1 + 2).")
        except Exception as _e:
            log_crash("IMPORT IMAGE ERROR", "Gagal menganalisis file galeri.",
                      e=_e, tb=_e.__traceback__)
            self._notify(f"Error: {_e}")

    def on_stop(self):
        self.stop_work()


def main():
    try:
        AntiTimpaMobileApp().run()
    except Exception as _e:
        log_crash(
            "TOP-LEVEL CRASH",
            "Exception di main() — app akan ditutup Kivy. Kirim log.",
            e=_e, tb=_e.__traceback__, always_print=True,
        )
        raise


if __name__ == "__main__":
    main()
