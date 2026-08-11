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
from kivy.uix.image import Image as KivyImage
from kivymd.app import MDApp
from kivymd.uix.label import MDLabel
from kivy.network.urlrequest import UrlRequest
import json
try:
    from plyer import gps
except Exception:
    gps = None

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
                text: "Mulai Kamera (L1 + L2)"
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
            self._skip = 0                 # throttle analisis (hemet CPU)

        # -- Per-frame camera analysis (thread analisis Camera4Kivy) ----------
        # Throttling: hanya sebagian frame yang dianalisis penuh (Layer1+Layer2).
        # Sisanya hanya dirender ke preview (ringan). Ini memangkas CPU besar.
        def analyze_pixels_callback(self, pixels, image_size, image_pos,
                                    image_scale, mirror):
            app = self._app
            if app is None or not app.dual:
                return
            w, h = image_size
            try:
                # pixels: RGBA byte buffer (w*h*4). Konversi ke BGR numpy.
                rgba = np.frombuffer(pixels, dtype=np.uint8).reshape((h, w, 4))
                bgr = cv2.cvtColor(rgba, cv2.COLOR_RGBA2BGR)

                # throttle: analisis penuh tiap 4 frame (hemet CPU, tetap responsif)
                do_analyze = (self._skip >= 3)
                if do_analyze:
                    self._skip = 0
                else:
                    self._skip += 1

                if do_analyze:
                    # Layani payload tepel dari kotak input, kalau ada.
                    force_raw = app.pasted_raw()
                    snap = app.core.process_frame(
                        bgr,
                        optical_type="physical_camera_scan",
                        force_raw=force_raw,
                        client_city=app.client_city,
                    )
                    hud = app._draw_hud(bgr, snap)
                    # Buat texture RGBA (thread-safe utk ditampilkan di UI).
                    out = cv2.cvtColor(hud, cv2.COLOR_BGR2RGBA)
                    self._set_texture(out.tobytes(), w, h)
                    # Perbarui kartu hasil (lewat main thread).
                    app.post_snapshot(snap)
                else:
                    # frame antara: tampilkan apa adanya (tanpa HUD) - lebih ringan
                    self._set_texture(rgba.tobytes(), w, h)

            except Exception as _e:
                # Error per-frame TIDAK fatal untuk app; catat ke log supaya
                # terlihat (mis. konversi warna, numpy, atau analisis gagal).
                # Tapi jangan spam: batasi frekuensi.
                pass

        @mainthread
        def _set_texture(self, rgba_bytes, w, h):
            # Bebaskan texture lama dulu agar tidak bocor RAM (penggantian
            # texture per-frame bisa membengkakkan memori kalau tidak dihapus).
            if self._frame_texture is not None:
                self._frame_texture = None
            tex = Texture.create(size=(w, h), colorfmt="rgba")

            tex.blit_buffer(rgba_bytes, colorfmt="rgba", bufferfmt="ubyte")

            tex.flip_vertical()

            self._frame_texture = tex

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
        if gps:
            try:
                gps.configure(on_location=self._on_location)
                gps.start(minTime=10000, minDistance=50) # every 10s or 50m
            except Exception as e:
                log_crash("GPS START ERROR", "Gagal start GPS.", e=e)

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
        
        url = f"https://nominatim.openstreetmap.org/reverse?lat={lat}&lon={lon}&format=json&zoom=10"
        UrlRequest(
            url,
            on_success=self._on_city_success,
            on_failure=self._on_city_fail,
            on_error=self._on_city_fail,
            req_headers={'User-Agent': 'AntiTimpaApp/0.1.0'}
        )

    def _on_city_success(self, req, result):
        self.is_fetching_city = False
        try:
            address = result.get("address", {})
            city = address.get("city") or address.get("town") or address.get("county")
            if city:
                self.client_city = city.upper()
        except Exception:
            pass

    def _on_city_fail(self, req, error):
        self.is_fetching_city = False

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
            # Kamera nyata sudah jalan via on_start; cukup perbarui hasil sekali.
            if isinstance(self._preview_widget, CameraLivePreview):
                self._set_status("Kamera aktif (Lokal). Arahkan ke QRIS.")
            return
        if not self.is_running:
            self.start_synthetic()
        else:
            self._tick(0)

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

    def _apply_risk_style(self, title_lbl, score, crc_valid):
        if not crc_valid:
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
        self._apply_risk_style(title, snap['combined_score'], l2.get('crc_valid', True))
        self._clear_box()

        self._add_row("Layer 1 - Skor Optik", f"{l1.get('l1_score', 0):.3f}")
        self._add_row("Layer 1 - Status", l1.get('risk_level', 'N/A'))
        self._add_row("Layer 2 - CRC-16", "VALID" if l2.get('crc_valid', False) else "GAGAL")
        self._add_row("Layer 2 - Merchant", l2.get('merchant_name') or 'N/A')
        self._add_row("Layer 2 - Kota", l2.get('merchant_city') or 'N/A')
        
        c_city = l3.get('client_city', 'N/A')
        self._add_row("Layer 3 - Lokasi Klien", c_city)
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
