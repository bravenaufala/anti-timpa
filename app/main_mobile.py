"""
Anti Timpa QRIS - Mobile App (dual-layer: Layer 1 OPTIK + Layer 2 EMVCo + kamera)

UI Modern sesuai desain HomePage 2.png:
- Background: Soft pastel sky blue (#DDF0FC)
- Header: Anti Timpa + subtitle "Aplikasi pendeteksi QRIS asli atau timpa"
- Top Card: Tampilan live camera (Camera4Kivy / CameraX / Synthetic) dengan sudut melengkung (radius 28dp)
- Action Row: Tombol "Shoot QR" dan tombol toggle "Tampilkan Log" / "Sembunyikan Log"
- Collapsible Log Section: Tampil / tersembunyi penuh berdasarkan state tombol log
- Bottom Card: Informasi Merchant, Lokasi, Skor Edge & L2, serta Circular Gauge "QRIS Detection Score" (83 / dinamis)
- Bottom Bar: Shutter button ganda (cyan) di tengah dan tombol Galeri di kanan
- Home Indicator bar di bagian bawah
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
    """Kirim pesan log ke logcat (sumber adb) dengan tag yang mudah difilter."""
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


def _install_crash_hooks():
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
        sys_excepthook(exc_type, exc, exc_tb)

    sys.excepthook = _handler

    try:
        import threading
        th_excepthook = threading.excepthook

        def _th_handler(args):
            try:
                log_crash(
                    "THREAD EXCEPTION [%s]" % args.name,
                    "Error di thread non-UI, tidak fatal.",
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
from kivy.graphics import Color, Rectangle, Line, Ellipse, RoundedRectangle
from kivy.graphics.texture import Texture
from kivy.metrics import dp
from kivy.network.urlrequest import UrlRequest
from kivy.uix.image import Image as KivyImage
from kivy.uix.widget import Widget
from kivy.uix.behaviors import ButtonBehavior
from kivy.properties import StringProperty, ListProperty, NumericProperty, BooleanProperty
from kivymd.app import MDApp
from kivymd.uix.label import MDLabel
from kivymd.uix.card import MDCard

try:
    from plyer import gps as _plyer_gps
    gps = _plyer_gps
except Exception:
    gps = None


def _monkeypatch_android_gps():
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

# Layer 2 is pure-Python (stdlib only).
from layer2_emvco import process_layer2_tlv

# Dual-layer pipeline with cv2/numpy
try:
    import cv2
    import numpy as np
    from scanner_core import QrisScannerCore
    from layer1_optical import expand_bounding_box
    import layer2_emvco

    DUAL_LAYER = True
    log_crash("INFO", "Dual-layer pipeline TERMUAT (cv2+numpy tersedia).")
except Exception as _e:
    log_crash(
        "IMPORT ERROR",
        "Gagal import cv2/numpy/scanner_core. Fallback ke Layer-2-only.",
        e=_e, tb=_e.__traceback__, always_print=True,
    )
    cv2 = None
    np = None
    DUAL_LAYER = False

# Camera4Kivy
try:
    from camera4kivy import Preview as C4KPreview
    CAMERA4KIVY = True
except Exception as _e:
    log_crash("INFO", "camera4kivy tidak tersedia — pakai feed sintetik.", e=_e)
    C4KPreview = None
    CAMERA4KIVY = False


# --------------------------------------------------------------------------
# Custom UI Widgets untuk mencocokkan desain HomePage 2.png
# --------------------------------------------------------------------------
class ScoreGaugeWidget(Widget):
    """Circular score gauge dengan lingkaran hijau dan teks angka besar di tengah."""
    score_text = StringProperty("83")
    gauge_color = ListProperty([0.18, 0.77, 0.40, 1.0])  # #2ECA66 (Green)


class ShutterButton(ButtonBehavior, Widget):
    """Tombol Shutter Kamera ganda: Lingkaran luar cyan + lingkaran dalam cyan cerah."""
    outer_color = ListProperty([0.0, 0.76, 0.96, 0.35])
    inner_color = ListProperty([0.0, 0.76, 0.96, 1.0])


# --------------------------------------------------------------------------
# Definisi KV String (UI Pixel-Perfect Sesuai HomePage 2.png)
# --------------------------------------------------------------------------
_KV_COMMON_HEADER = '''
<ScoreGaugeWidget>:
    size_hint: None, None
    size: "108dp", "108dp"
    canvas:
        Color:
            rgba: self.gauge_color
        Line:
            circle: (self.center_x, self.center_y, dp(44))
            width: dp(4.5)
    Label:
        center_x: root.center_x
        center_y: root.center_y
        text: root.score_text
        font_size: "38sp"
        bold: True
        color: root.gauge_color

<ShutterButton>:
    size_hint: None, None
    size: "72dp", "72dp"
    canvas:
        Color:
            rgba: self.outer_color
        Line:
            circle: (self.center_x, self.center_y, dp(32))
            width: dp(3.5)
        Color:
            rgba: self.inner_color
        Ellipse:
            pos: (self.center_x - dp(24), self.center_y - dp(24))
            size: (dp(48), dp(48))

MDScreen:
    md_bg_color: 0.867, 0.941, 0.988, 1

    MDBoxLayout:
        orientation: "vertical"
        padding: ["16dp", "10dp", "16dp", "12dp"]
        spacing: "10dp"

        # --- Top Header ---
        MDBoxLayout:
            orientation: "vertical"
            adaptive_height: True
            spacing: "2dp"
            padding: [0, "6dp", 0, "4dp"]

            MDLabel:
                text: "Anti Timpa"
                bold: True
                halign: "center"
                font_size: "24sp"
                theme_text_color: "Custom"
                text_color: 0.07, 0.09, 0.13, 1
                size_hint_y: None
                height: self.texture_size[1]

            MDLabel:
                text: "Aplikasi pendeteksi QRIS asli atau timpa"
                halign: "center"
                font_size: "13.5sp"
                theme_text_color: "Custom"
                text_color: 0.45, 0.49, 0.55, 1
                size_hint_y: None
                height: self.texture_size[1]

        # --- Top Card: Camera View Area ---
        MDCard:
            id: camera_card
            radius: [dp(28), dp(28), dp(28), dp(28)]
            md_bg_color: 0.953, 0.961, 0.969, 1
            elevation: 0
            size_hint_y: None
            height: "230dp"
            padding: 0
            clip_children: True

            MDFloatLayout:
'''

_KV_PREVIEW_REAL = '''
                CameraLivePreview:
                    id: preview
                    size_hint: 1, 1
                    pos_hint: {"center_x": 0.5, "center_y": 0.5}
'''

_KV_PREVIEW_SYN = '''
                SyntheticPreview:
                    id: preview
                    size_hint: 1, 1
                    pos_hint: {"center_x": 0.5, "center_y": 0.5}
                    allow_stretch: True
                    keep_ratio: True
'''

_KV_PREVIEW_NONE = '''
                Widget:
                    id: preview
                    size_hint: 1, 1
'''

_KV_COMMON_BODY = '''
                MDLabel:
                    id: camera_hint_label
                    text: "Arahkan ke QRIS anda untuk mendeteksi..."
                    halign: "center"
                    valign: "center"
                    font_size: "13sp"
                    theme_text_color: "Custom"
                    text_color: 0.52, 0.55, 0.60, 1
                    pos_hint: {"center_x": 0.5, "center_y": 0.5}
                    opacity: 0.75

        # --- Action Buttons Row ---
        MDBoxLayout:
            orientation: "horizontal"
            adaptive_height: True
            spacing: "8dp"
            padding: [0, 0, "4dp", 0]

            Widget:
                size_hint_x: 1

            MDFillRoundFlatButton:
                id: toggle_log_btn
                text: "Tampilkan Log"
                md_bg_color: 0.55, 0.56, 0.58, 1
                theme_text_color: "Custom"
                text_color: 1, 1, 1, 1
                font_size: "12sp"
                size_hint_y: None
                height: "36dp"
                on_release: app.toggle_log()

        # --- Collapsible Log Section (Tersembunyi Penuh / Tampil Berdasarkan State) ---
        MDCard:
            id: log_card
            size_hint_y: None
            height: 0
            opacity: 0
            disabled: True
            radius: [dp(20), dp(20), dp(20), dp(20)]
            md_bg_color: 0.12, 0.14, 0.18, 1
            padding: "10dp"
            elevation: 0

            ScrollView:
                MDLabel:
                    id: log_detail
                    text: "Log detail scan akan tampil di sini (dan terkirim ke adb logcat)."
                    theme_text_color: "Custom"
                    text_color: 0.75, 0.95, 0.75, 1
                    font_style: "Caption"
                    size_hint_y: None
                    height: self.texture_size[1]
                    text_size: self.width, None
                    halign: "left"
                    valign: "top"

        # --- Bottom Card: Result & Score Display ---
        MDCard:
            id: result_card
            radius: [dp(28), dp(28), dp(28), dp(28)]
            md_bg_color: 0.953, 0.961, 0.969, 1
            elevation: 0
            size_hint_y: 1
            padding: ["20dp", "14dp", "20dp", "14dp"]
            orientation: "vertical"
            spacing: "4dp"

            MDBoxLayout:
                id: info_box
                orientation: "vertical"
                adaptive_height: True
                spacing: "3dp"

                MDLabel:
                    id: merchant_name_label
                    text: "Nama Merchant: -"
                    bold: True
                    font_size: "13.5sp"
                    theme_text_color: "Custom"
                    text_color: 0.07, 0.09, 0.13, 1
                    size_hint_y: None
                    height: self.texture_size[1]

                MDLabel:
                    id: merchant_loc_label
                    text: "Lokasi Merchant: -"
                    bold: True
                    font_size: "13.5sp"
                    theme_text_color: "Custom"
                    text_color: 0.07, 0.09, 0.13, 1
                    size_hint_y: None
                    height: self.texture_size[1]

                MDLabel:
                    id: edge_score_label
                    text: "Edge Detection Score: -"
                    bold: True
                    font_size: "13.5sp"
                    theme_text_color: "Custom"
                    text_color: 0.07, 0.09, 0.13, 1
                    size_hint_y: None
                    height: self.texture_size[1]

                MDLabel:
                    id: l2_score_label
                    text: "L2 Score: -"
                    bold: True
                    font_size: "13.5sp"
                    theme_text_color: "Custom"
                    text_color: 0.07, 0.09, 0.13, 1
                    size_hint_y: None
                    height: self.texture_size[1]

            Widget:
                size_hint_y: 0.1

            MDLabel:
                id: qris_value_label
                text: "Nilai QRIS: -"
                bold: False
                halign: "center"
                font_size: "13sp"
                theme_text_color: "Custom"
                text_color: 0.10, 0.12, 0.16, 1
                size_hint_y: None
                height: dp(28)
                text_size: self.width, None

            MDLabel:
                id: score_title_label
                text: "QRIS Detection Score"
                bold: True
                halign: "center"
                font_size: "14.5sp"
                theme_text_color: "Custom"
                text_color: 0.07, 0.09, 0.13, 1
                size_hint_y: None
                height: self.texture_size[1]

            MDBoxLayout:
                orientation: "vertical"
                adaptive_height: True
                spacing: "2dp"

                ScoreGaugeWidget:
                    id: score_gauge
                    pos_hint: {"center_x": 0.5}

                MDLabel:
                    id: risk_status_badge
                    text: "SIAP MEMINDAI"
                    bold: True
                    halign: "center"
                    font_size: "12sp"
                    theme_text_color: "Custom"
                    text_color: 0.45, 0.49, 0.55, 1
                    size_hint_y: None
                    height: self.texture_size[1]

        # --- Bottom Floating Controls Bar ---
        MDFloatLayout:
            size_hint_y: None
            height: "72dp"

            ShutterButton:
                id: shutter_btn
                size_hint: None, None
                size: "72dp", "72dp"
                pos_hint: {"center_x": 0.5, "center_y": 0.5}
                on_release: app.start_or_analyze()

            MDIconButton:
                icon: "image"
                icon_size: "38dp"
                theme_text_color: "Custom"
                text_color: 0.0, 0.76, 0.96, 1
                size_hint: None, None
                size: "48dp", "48dp"
                pos_hint: {"center_x": 0.9, "center_y": 0.5}
                on_release: app.import_image()

        # --- Bottom Home Indicator Bar ---
        Widget:
            size_hint: None, None
            size: "134dp", "4.5dp"
            pos_hint: {"center_x": 0.5}
            canvas:
                Color:
                    rgba: 0.08, 0.09, 0.12, 1
                RoundedRectangle:
                    pos: self.pos
                    size: self.size
                    radius: [dp(2.25), dp(2.25), dp(2.25), dp(2.25)]
'''

_KV_REAL = _KV_COMMON_HEADER + _KV_PREVIEW_REAL + _KV_COMMON_BODY
_KV_SYN = _KV_COMMON_HEADER + _KV_PREVIEW_SYN + _KV_COMMON_BODY
_KV_NONE = _KV_COMMON_HEADER + _KV_PREVIEW_NONE + _KV_COMMON_BODY


# --------------------------------------------------------------------------
# Widget kamera nyata (subclass dari Camera4Kivy Preview)
# --------------------------------------------------------------------------
if CAMERA4KIVY:
    class CameraLivePreview(C4KPreview):
        """Analyze real camera frames through the dual-layer engine.

        Live video dirender NATIVELY oleh Camera4Kivy sehingga orientasi dan
        mirror sudah benar serta mengisi penuh widget (via configure_viewport).
        Widget ini hanya menangkap frame untuk analisis QR on-demand dan,
        saat foto diambil, menggambar overlay HUD (kotak deteksi) di atasnya.

        Perbaikan bug: dulu terdapat overlay `_frame_texture` yang melakukan
        blit + flip_vertical() pada SETIAP frame. Karena flip_vertical() di
        Kivy bersifat meng-toggle koordinat UV (bukan membalik data), memanggil
        tiap frame membuat orientasi berganti-ganti tiap frame -> video selalu
        terbalik vertikal & tampak glitch. Overlay itu dihapus; preview live
        cukup ditangani native, dan flip hanya dilakukan sekali di
        show_frame_oneshot() saat membuat tekstur overlay.
        """

        def __init__(self, app_ref=None, **kwargs):
            super().__init__(**kwargs)
            self._app = app_ref
            self._last_pixels = None
            self._last_size = None
            self._hud_rect = None
            self._hud_texture = None
            # Posisi/ukuran area gambar dalam widget (untuk menempatkan overlay HUD).
            self._preview_region = (0, 0, 0, 0)

        def analyze_pixels_callback(self, pixels, image_size, image_pos,
                                    image_scale, mirror):
            app = self._app
            if app is None or not app.dual:
                return
            # Hanya simpan frame terbaru untuk analisis QR on-demand (take_photo).
            # Preview live sudah dirender native oleh Camera4Kivy.
            self._last_pixels = pixels
            self._last_size = image_size

        def canvas_instructions_callback(self, texture, tex_size, tex_pos):
            # Jangan menggambar overlay per-frame (agar tidak glitch).
            # Cukup catat area gambar untuk keperluan overlay HUD pada foto.
            if tex_size and tex_pos:
                self._preview_region = (tex_pos[0], tex_pos[1],
                                        tex_size[0], tex_size[1])

        @mainthread
        def show_frame_oneshot(self, frame_bgr):
            """Tampilkan satu hasil analisis (HUD) di atas preview live.

            Tekstur dibuat & di-flip HANYA SEKALI sehingga orientasi stabil.
            """
            try:
                h, w = frame_bgr.shape[:2]
                rgba = cv2.cvtColor(frame_bgr, cv2.COLOR_BGR2RGBA)
                tex = Texture.create(size=(w, h), colorfmt="rgba")
                tex.blit_buffer(rgba.tobytes(), colorfmt="rgba", bufferfmt="ubyte")
                tex.flip_vertical()  # sekali saja: orientasi menjadi benar & tetap.
                self._hud_texture = tex
                px, py, pw, ph = self._preview_region
                if pw <= 0 or ph <= 0:
                    px, py, pw, ph = self.pos[0], self.pos[1], self.width, self.height
                self.canvas.after.clear()
                with self.canvas.after:
                    Color(1, 1, 1, 1)
                    self._hud_rect = Rectangle(texture=tex, pos=(px, py),
                                               size=(pw, ph))
            except Exception:
                pass

        def configure_viewport(self):
            """Fill seluruh area widget: preview menutupi seluruh div (center-crop)."""
            self.view_size = (round(self.width), round(self.height))
            self.view_pos = [self.pos[0], self.pos[1]]

        def connect(self):
            self.connect_camera(enable_analyze_pixels=True,
                                enable_video=False,
                                analyze_pixels_resolution=480)

        def disconnect(self):
            self.disconnect_camera()
else:
    class CameraLivePreview(object):
        _app = None

        def __init__(self, *a, **k):
            raise RuntimeError("camera4kivy tidak tersedia di build ini")

        def connect(self):
            raise RuntimeError("camera4kivy tidak tersedia di build ini")

        def disconnect(self):
            pass


# --------------------------------------------------------------------------
# Preview sintetik (Image) -- fallback bila kamera nyata tidak tersedia.
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
        self.core = None
        self.is_running = False
        self.sim_count = 0
        self.notify_ev = None
        self._preview_widget = None
        self._snapshot = None
        self._snap_scheduled = False
        self.show_log = False  # State Tampilkan/Sembunyikan Log

        self.client_city = None
        self.last_gps_coords = None
        self.is_fetching_city = False
        self._captured = False
        self._processing = False

        if self.dual:
            self.core = QrisScannerCore(blur_threshold=100.0, fifo_size=5)

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------
    def build(self):
        self.theme_cls.theme_style = "Light"
        self.theme_cls.primary_palette = "LightBlue"

        if self.dual and self.camera_ok:
            kv = _KV_REAL
        elif self.dual:
            kv = _KV_SYN
        else:
            kv = _KV_NONE

        root = Builder.load_string(kv)
        self.root = root

        if self.dual and self.camera_ok:
            pw = self.root.ids.get("preview")
            if isinstance(pw, CameraLivePreview):
                pw._app = self
                self._preview_widget = pw

        self._apply_log_visibility()
        # Saat belum memfoto (default) jangan tampilkan log maupun skor QR.
        self._reset_result_ui()

        return self.root

    def _start_camera(self):
        if isinstance(self._preview_widget, CameraLivePreview):
            try:
                self._preview_widget.connect()
            except Exception as e:
                log_crash("CAMERA CONNECT ERROR", "Gagal membuka kamera.", e=e,
                          tb=e.__traceback__, always_print=True)
        self.is_running = True

    def on_start(self):
        self._reset_result_ui()
        self._show_latest_log()
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
        Clock.schedule_once(lambda dt: self._start_camera(), 0.0)

    def _granted(self, permissions):
        self._start_gps()
        Clock.schedule_once(lambda dt: self._start_camera(), 0.0)

    def _start_gps(self):
        if gps:
            try:
                gps.configure(on_location=self._on_location)
                gps.start(minTime=10000, minDistance=50)
            except Exception as e:
                log_crash("GPS START ERROR", "Gagal start GPS.", e=e)
        self._try_last_known_location()

    def _try_last_known_location(self):
        if self.client_city and self.client_city != "LOADING":
            return
        try:
            from jnius import autoclass
            from plyer.platforms.android import activity
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
        except Exception as e:
            _log_to_adb("ANTITIMPA", "GEOCODE PARSE ERROR: %s" % str(e))

    def _on_city_fail(self, req, error):
        self.is_fetching_city = False
        _log_to_adb("ANTITIMPA", "GEOCODE FAIL: %s" % str(error))

    @mainthread
    def _notify(self, msg):
        """Tampilkan pesan status pada log umum. Dijalankan di main-thread
        UI agar aman dipanggil dari thread analisis (take_photo)."""
        log_crash("NOTIFY", str(msg))
        try:
            log_lbl = self.root.ids.get("log_detail")
            if log_lbl:
                log_lbl.text = f"[{datetime.now().strftime('%H:%M:%S')}] {msg}\n" + log_lbl.text
        except Exception:
            pass

    def _show_latest_log(self):
        try:
            path = self.log_path()
            if not path or not os.path.exists(path):
                return
            with open(path, "r") as f:
                lines = f.read().splitlines()
            important = [l for l in lines
                         if any(k in l.upper() for k in
                                ("ERROR", "EXCEPTION", "GAGAL", "FAIL", "IMPORT"))]
            if important:
                self._notify("Error startup:\n" + "\n".join(important[-4:]))
        except Exception:
            pass

    @staticmethod
    def log_path():
        return LOG_PATH or _log_path()

    # ------------------------------------------------------------------
    # Toggle Log Section (Visible / Hidden Sepenuhnya)
    # ------------------------------------------------------------------
    def toggle_log(self):
        """Menampilkan atau menyembunyikan section log secara penuh."""
        self.show_log = not self.show_log
        log_card = self.root.ids.get('log_card')
        toggle_btn = self.root.ids.get('toggle_log_btn')

        if log_card:
            if self.show_log:
                log_card.height = dp(140)
                log_card.opacity = 1
                log_card.disabled = False
                if toggle_btn:
                    toggle_btn.text = "Sembunyikan Log"
            else:
                log_card.height = 0
                log_card.opacity = 0
                log_card.disabled = True
                if toggle_btn:
                    toggle_btn.text = "Tampilkan Log"

        self._apply_log_visibility()

    def _apply_log_visibility(self):
        """Atur visibilitas area hasil & log:
        - log (info_box + log_card) tampil penuh hanya saat show_log aktif;
        - nilai QRIS & gauge skor hanya muncul setelah ada hasil scan
          (self._captured == True), sehingga sebelum memfoto semuanya kosong."""
        ids = self.root.ids if self.root else {}
        show_full = bool(getattr(self, 'show_log', False))
        has_result = bool(getattr(self, '_captured', False))
        if 'info_box' in ids:
            ids.info_box.opacity = 1 if show_full else 0
            ids.info_box.disabled = not show_full
        if 'score_title_label' in ids:
            ids.score_title_label.opacity = 1 if (show_full and has_result) else 0
        # Nilai QRIS & gauge skor hanya muncul setelah ada hasil scan.
        if 'qris_value_label' in ids:
            ids.qris_value_label.opacity = 1 if has_result else 0
        if 'score_gauge' in ids:
            ids.score_gauge.opacity = 1 if has_result else 0
        if 'risk_status_badge' in ids:
            ids.risk_status_badge.opacity = 1 if has_result else 0
        if 'log_card' in ids:
            ids.log_card.opacity = 1 if show_full else 0

    @mainthread
    def _reset_result_ui(self):
        """Kosongkan seluruh area hasil & log (state sebelum memfoto).
        Dipanggil saat aplikasi pertama kali dibuka supaya belum ada foto
        tidak menampilkan log detail maupun skor QR."""
        ids = self.root.ids if self.root else {}
        defaults = {
            'merchant_name_label': 'Nama Merchant: -',
            'merchant_loc_label': 'Lokasi Merchant: -',
            'edge_score_label': 'Edge Detection Score: -',
            'l2_score_label': 'L2 Score: -',
        }
        for name, text in defaults.items():
            if name in ids:
                ids[name].text = text
        if 'qris_value_label' in ids:
            ids.qris_value_label.text = 'Nilai QRIS: -'
        if 'score_gauge' in ids:
            ids.score_gauge.score_text = ''
        if 'risk_status_badge' in ids:
            ids.risk_status_badge.text = ''
        hint_lbl = ids.get('camera_hint_label')
        if hint_lbl:
            hint_lbl.opacity = 0.75
        log_lbl = ids.get('log_detail')
        if log_lbl:
            log_lbl.text = ''
        self._apply_log_visibility()

    # ------------------------------------------------------------------
    # Snapshot rendering
    # ------------------------------------------------------------------
    def post_snapshot(self, snap):
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
    # Entry Action: Shoot QR / Shutter
    # ------------------------------------------------------------------
    def start_or_analyze(self):
        """Trigger deteksi QR dari kamera terbaru / synthetic feed."""
        if not self.dual:
            self._notify("Mode Dual-Layer tidak aktif.")
            return
        if self.camera_ok:
            self.take_photo()
            return
        if not self.is_running:
            self.start_synthetic()
        else:
            self._tick(0)

    def take_photo(self):
        """One-shot: ambil frame kamera terakhir lalu analisis QRIS."""
        if self._processing:
            self._notify("Analisis sedang berjalan, mohon tunggu...")
            return

        pw = self._preview_widget
        if pw is None or getattr(pw, "_last_pixels", None) is None:
            _log_to_adb("ANTITIMPA", "take_photo: belum ada frame kamera")
            self._notify("Kamera belum siap menerima frame.")
            return

        self._processing = True
        _log_to_adb("ANTITIMPA", "TAKE PHOTO / SHOOT QR START")
        self._notify("Mengambil & menganalisis QR dari kamera...")

        def _work():
            try:
                # Ambil snapshot terbaru dari kamera (RGBA bytes) di dalam thread
                # agar konversi & analisis tidak memblokir UI.
                frame_bgr = self._pixels_to_bgr()
                if frame_bgr is None:
                    self._notify("Frame kamera tidak valid.")
                    return

                try:
                    _log_to_adb("ANTITIMPA", "TAKE PHOTO frame shape=%s dtype=%s mean=%.1f min=%.1f max=%.1f" % (
                        frame_bgr.shape, frame_bgr.dtype,
                        float(frame_bgr.mean()), float(frame_bgr.min()), float(frame_bgr.max())))
                except Exception:
                    pass

                result = QrisScannerCore.analyze_image(
                    frame_bgr, optical_type="physical_camera_scan",
                    client_city=self.client_city)

                if result.get('ok') and result.get('raw'):
                    self._present_result(result['snapshot'], frame_bgr)
                else:
                    _log_to_adb("ANTITIMPA", "NO QR TERDETEKSI")
                    self._captured = False
                    self._reset_result_ui()
                    self._notify("Tidak ada QR terbaca pada frame. Arahkan lebih dekat.")
            except Exception as _e:
                log_crash("TAKE PHOTO ERROR", str(_e), e=_e,
                          tb=_e.__traceback__, always_print=True)
            finally:
                self._processing = False

        threading.Thread(target=_work, daemon=True).start()

    def _pixels_to_bgr(self):
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

    @mainthread
    def _present_result(self, snap, frame_bgr):
        self._captured = True
        self._snapshot = snap
        self.post_snapshot(snap)
        try:
            pw = self._preview_widget
            if frame_bgr is not None and pw is not None and hasattr(pw, 'show_frame_oneshot'):
                pw.show_frame_oneshot(self._draw_hud(frame_bgr, snap))
        except Exception:
            pass
        self._notify("Analisis QR selesai.")

    # ------------------------------------------------------------------
    # Synthetic Feed (Fallback demo)
    # ------------------------------------------------------------------
    def start_synthetic(self):
        if self.is_running:
            return
        self.is_running = True
        self.sim_count = 0
        self.notify_ev = Clock.schedule_interval(self._tick, 1.0 / 30.0)

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

    def _synthetic_frame(self):
        frame = np.full((480, 640, 3), 235, dtype=np.uint8)
        qx, qy, qw, qh = 200, 140, 240, 240
        cv2.rectangle(frame, (qx - 20, qy - 20), (qx + qw + 20, qy + qh + 20), (245, 245, 245), -1)
        cv2.rectangle(frame, (qx, qy), (qx + qw, qy + qh), (0, 0, 0), 4)
        cv2.rectangle(frame, (qx + 10, qy + 10), (qx + 60, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + qw - 60, qy + 10), (qx + qw - 10, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + 10, qy + qh - 60), (qx + 60, qy + qh - 10), (0, 0, 0), -1)

        if (self.sim_count // 60) % 2 == 1:
            cv2.rectangle(frame, (qx - 15, qy - 15), (qx + qw + 15, qy + qh + 15), (30, 30, 30), 4)
            cv2.rectangle(frame, (qx - 10, qy - 10), (qx + qw + 10, qy + qh + 10), (170, 170, 170), 2)

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

        snap = self.core.process_frame(
            frame, optical_type="physical_camera_scan",
            force_bbox=force_bbox, force_raw=force_raw,
            client_city=self.client_city,
        )
        hud = self._draw_hud(frame, snap)
        self._show_preview(hud)
        self._render_snapshot(snap)

    # ------------------------------------------------------------------
    # HUD & Render Output
    # ------------------------------------------------------------------
    def _draw_hud(self, frame, snap):
        display = frame.copy()
        if snap.get('qr_bbox') is not None and not snap.get('is_blurry'):
            x, y, bw, bh = snap['qr_bbox']
            box_color = self._risk_color(snap.get('combined_score', 0), snap.get('l2', {}).get('crc_valid', True))
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

    def _render_snapshot(self, snap):
        """Render hasil deteksi ke UI sesuai komponen HomePage 2.png."""
        if not self.root:
            return

        l1 = snap.get('l1', {})
        l2 = snap.get('l2', {})
        l3 = snap.get('l3', {})
        crc_valid = l2.get('crc_valid', True)
        combined_score = snap.get('combined_score', 0.0)
        risk_level = snap.get('combined_risk_level', 'NO QR')

        merchant_name = l2.get('merchant_name') or '-'
        merchant_loc = l3.get('merchant_city') or l2.get('merchant_city') or '-'
        edge_density = l1.get('spatial_edge_density', 0.0)
        l1_risk = l1.get('risk_level', '-')
        l2_score_val = l2.get('l2_score', 0.0)

        # 1. Update baris teks informasi pada Bottom Card
        ids = self.root.ids
        if 'merchant_name_label' in ids:
            ids.merchant_name_label.text = f"Nama Merchant: {merchant_name}"
        if 'merchant_loc_label' in ids:
            ids.merchant_loc_label.text = f"Lokasi Merchant: {merchant_loc}"
        if 'edge_score_label' in ids:
            ids.edge_score_label.text = f"Edge Detection Score: {edge_density:.4f} ({l1_risk})"
        if 'l2_score_label' in ids:
            crc_str = "VALID CRC" if crc_valid else "INVALID CRC"
            ids.l2_score_label.text = f"L2 Score: {l2_score_val:.2f} ({crc_str})"

        qris_val = snap.get('raw_qris_str') or '(belum ada QR terdeteksi)'
        if 'qris_value_label' in ids:
            ids.qris_value_label.text = f"Nilai QRIS: {qris_val}"

        # Terapkan visibilitas sesuai state Tampilkan/Sembunyikan Log
        self._apply_log_visibility()

        # Sembunyikan placeholder hint jika QR terdeteksi
        hint_lbl = ids.get('camera_hint_label')
        if hint_lbl:
            hint_lbl.opacity = 0 if snap.get('raw_qris_str') else 0.75

        # 2. Hitung Trust / Safety Score (0 - 100) dan warna Circular Gauge
        if not crc_valid:
            score_num = 0
            gauge_col = [0.94, 0.27, 0.27, 1.0]  # Red
            badge_text = "HIGH RISK (CRC Gagal)"
        elif risk_level in ("NO QR", "MENUNGGU SCAN"):
            score_num = 83  # Default estetik sesuai mockup
            gauge_col = [0.18, 0.77, 0.40, 1.0]  # Green #2ECA66
            badge_text = "SIAP MEMINDAI"
        elif combined_score < 0.35:
            score_num = int(round(max(0.0, min(1.0, 1.0 - combined_score)) * 100))
            gauge_col = [0.18, 0.77, 0.40, 1.0]  # Green #2ECA66
            badge_text = "ASLI / AMAN (LOW RISK)"
        elif combined_score <= 0.70:
            score_num = int(round(max(0.0, min(1.0, 1.0 - combined_score)) * 100))
            gauge_col = [0.96, 0.62, 0.07, 1.0]  # Orange/Yellow
            badge_text = "WASPADA (CAUTION)"
        else:
            score_num = int(round(max(0.0, min(1.0, 1.0 - combined_score)) * 100))
            gauge_col = [0.94, 0.27, 0.27, 1.0]  # Red
            badge_text = "TERINDIKASI TIMPA (HIGH RISK)"

        gauge = ids.get('score_gauge')
        if gauge:
            gauge.score_text = str(score_num)
            gauge.gauge_color = gauge_col

        badge = ids.get('risk_status_badge')
        if badge:
            badge.text = badge_text
            badge.text_color = gauge_col

        # 3. Update isi log detail (hanya log umum — tanpa baris teknis/system)
        try:
            log_lbl = ids.get("log_detail")
            if log_lbl:
                log_lbl.text = self._general_scan_log(snap)
        except Exception:
            pass

    def _general_scan_log(self, snap):
        """Sajikan log UMUM saja (hasil scan QRIS) untuk UI — menyembunyikan
        baris teknis/system seperti blur var, L1 edge/glare, dan TLV intern."""
        l2 = snap.get('l2', {})
        l3 = snap.get('l3', {})
        crc_valid = l2.get('crc_valid', False)
        crc_txt = "VALID" if crc_valid else "GAGAL"

        qris_city = (l3.get('merchant_city') or l2.get('merchant_city') or None)
        client_city = l3.get('client_city')

        lines = [
            "===== SCAN %s ====" % datetime.now().strftime("%H:%M:%S"),
            "Nilai QRIS: %s" % (snap.get('raw_qris_str') or '(kosong)'),
            "Merchant: %s" % (l2.get('merchant_name') or "N/A"),
            "Lokasi QRIS: %s" % (qris_city or "N/A"),
            "Lokasi Anda: %s" % (client_city or "TIDAK ADA"),
            "Status CRC: %s" % crc_txt,
            "Skor Risiko: %.0f%%" % (round(max(0.0, min(1.0, snap.get('combined_score', 0))) * 100)),
            "Tingkat: %s" % snap.get('combined_risk_level', 'N/A'),
        ]
        warnings = (l2.get('warnings') or []) + (l3.get('warnings') or [])
        if warnings:
            lines.append("Peringatan: %s" % "; ".join(warnings))
        return "\n".join(lines)

    # ------------------------------------------------------------------
    # Import Gambar dari Galeri
    # ------------------------------------------------------------------
    def import_image(self):
        try:
            from plyer import filechooser
        except Exception as _e:
            log_crash("PLYER ERROR", "filechooser tidak tersedia.", e=_e)
            self._notify("File picker tidak tersedia di platform ini.")
            return
        if self.dual:
            self._notify("Membuka galeri...")
            filechooser.open_file(on_selection=self._on_file_selected)
        else:
            self._notify("Import gambar butuh OpenCV.")

    def _on_file_selected(self, selection):
        if selection:
            self._analyze_imported_file(selection[0])

    def _analyze_imported_file(self, path):
        try:
            frame = cv2.imread(path)
            if frame is None:
                self._notify("Gagal membaca file gambar.")
                return
            result = QrisScannerCore.analyze_image(frame, optical_type="imported_image")
            if not result.get('ok'):
                self._notify(result.get('error', "Tidak ada QR terdeteksi."))
                return
            self._present_result(result['snapshot'], frame)
            self._notify("Analisis file gambar selesai.")
        except Exception as _e:
            log_crash("IMPORT IMAGE ERROR", "Gagal menganalisis gambar.",
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
            "Exception di main() — app akan ditutup Kivy.",
            e=_e, tb=_e.__traceback__, always_print=True,
        )
        raise


if __name__ == "__main__":
    main()
