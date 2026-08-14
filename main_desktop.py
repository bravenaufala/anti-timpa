"""
Anti Timpa QRIS - KivyMD Cross-Platform App (desktop + Android + iOS)

Fully LOCAL / on-device scanner:
  * Analysis engine (QrisScannerCore) runs on device — no network, no cloud.
  * Desktop uses the OpenCV webcam (cv2.VideoCapture).
  * Mobile uses a Kivy Camera widget via camera-provider when available, and
    falls back to the synthetic demo generator so the app always works.

UI: KivyMD Material Design cards, live camera preview, risk HUD, and an
"import image" action for one-shot Light Layer 1 + Layer 2 validation.
"""

import os
import sys

# ---- Make analysis modules importable regardless of CWD ----
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import cv2
import numpy as np
from kivy.clock import Clock
from kivy.lang import Builder
from kivy.utils import platform
from kivy.uix.image import Image
from kivy.graphics.texture import Texture
from kivymd.app import MDApp
from kivymd.uix.snackbar import Snackbar

from scanner_core import QrisScannerCore
from layer1_optical import expand_bounding_box

# --------------------------------------------------------------------------
# KV - UI layout (Material Design)
# --------------------------------------------------------------------------
KV = '''
<ResultCard@MDCard>:
    padding: "12dp"
    size_hint_y: None
    height: "auto"
    md_bg_color: 0.12, 0.13, 0.16, 1

MDScreen:
    md_bg_color: 0.07, 0.08, 0.10, 1

    MDTopAppBar:
        id: topbar
        title: "Anti Timpa QRIS Scanner"
        md_bg_color: 0.12, 0.31, 0.71, 1
        left_action_items: [["menu"]]

    MDFloatLayout:

        # ---------- Live camera preview ----------
        CameraPreview:
            id: preview
            pos_hint: {"top": 1.0}
            size_hint: 1, 0.52
            bg_color: 0.10, 0.11, 0.14, 1

        # Status banner over the preview
        MDLabel:
            id: banner_label
            text: "Mencari QRIS... (tahan kamera tetap tenang)"
            pos_hint: {"top": 0.985}
            halign: "center"
            theme_text_color: "Custom"
            text_color: 1, 1, 1, 1
            bold: True

        # ---------- Results panel ----------
        ScrollView:
            pos_hint: {"top": 0.48}
            size_hint: 1, 0.52
            do_scroll_x: False
            MDBoxLayout:
                orientation: "vertical"
                adaptive_height: True
                padding: "12dp"
                spacing: "8dp"

                ResultCard:
                    id: risk_card
                    MDBoxLayout:
                        orientation: "vertical"
                        adaptive_height: True
                        md_bg_color: 0.0, 0.0, 0.0, 0.0
                        MDLabel:
                            id: risk_level_label
                            text: "BELUM ADA SCAN"
                            theme_text_color: "Custom"
                            text_color: 1, 1, 1, 1
                            font_style: "H5"
                            bold: True
                            halign: "center"
                        MDLabel:
                            id: risk_score_label
                            text: "Score Gabungan: —"
                            theme_text_color: "Custom"
                            text_color: 0.85, 0.88, 0.92, 1
                            halign: "center"
                        MDLabel:
                            id: risk_warnings_label
                            text: ""
                            theme_text_color: "Custom"
                            text_color: 1.0, 0.65, 0.35, 1
                            font_style: "Caption"
                            halign: "center"

                ResultCard:
                    MDBoxLayout:
                        orientation: "vertical"
                        adaptive_height: True
                        md_bg_color: 0.0, 0.0, 0.0, 0.0
                        MDLabel:
                            text: "Layer 1 - Tampilan Fisik (Optik)"
                            bold: True
                            theme_text_color: "Custom"
                            text_color: 0.6, 0.8, 1.0, 1
                        MDLabel:
                            id: l1_text
                            text: "Edge Density: —  |  Glare Var: —  |  Skor: —"
                            theme_text_color: "Custom"
                            text_color: 0.85, 0.88, 0.92, 1
                            font_style: "Caption"

                ResultCard:
                    MDBoxLayout:
                        orientation: "vertical"
                        adaptive_height: True
                        md_bg_color: 0.0, 0.0, 0.0, 0.0
                        MDLabel:
                            text: "Layer 2 - Payload EMVCo"
                            bold: True
                            theme_text_color: "Custom"
                            text_color: 1.0, 0.8, 0.6, 1
                        MDLabel:
                            id: l2_text
                            text: "CRC: —  |  Merchant: —  |  MCC: —  |  Kota: —"
                            theme_text_color: "Custom"
                            text_color: 0.85, 0.88, 0.92, 1
                            font_style: "Caption"
                        MDLabel:
                            id: l2_mode_text
                            text: "Mode: —  |  Payload Format: —"
                            theme_text_color: "Custom"
                            text_color: 0.75, 0.78, 0.82, 1
                            font_style: "Caption"

                    ResultCard:
                        MDBoxLayout:
                            orientation: "vertical"
                            adaptive_height: True
                            md_bg_color: 0.0, 0.0, 0.0, 0.0
                            MDLabel:
                                text: "Layer 3 - Kota Geofence"
                                bold: True
                                theme_text_color: "Custom"
                                text_color: 0.8, 0.85, 0.55, 1
                            MDLabel:
                                id: l3_text
                                text: "Kota Klien: —  |  Kota Merchant: —  |  Skor: —"
                                theme_text_color: "Custom"
                                text_color: 0.85, 0.88, 0.92, 1
                                font_style: "Caption"
                            MDLabel:
                                id: l3_status_text
                                text: "Status: —"
                                theme_text_color: "Custom"
                                text_color: 1.0, 0.65, 0.55, 1
                                font_style: "Caption"

                    ResultCard:
                        MDBoxLayout:
                            orientation: "vertical"
                            adaptive_height: True
                            md_bg_color: 0.0, 0.0, 0.0, 0.0
                            MDLabel:
                                text: "Status Feed"
                                bold: True
                                theme_text_color: "Custom"
                                text_color: 0.75, 0.85, 0.75, 1
                            MDLabel:
                                id: status_text
                                text: "Kamera: menunggu..."
                                theme_text_color: "Custom"
                                text_color: 0.85, 0.88, 0.92, 1
                                font_style: "Caption"

                    # ---------- Layer 3 (desktop) client city input ----------
                    MDBoxLayout:
                        size_hint_y: None
                        height: "48dp"
                        spacing: "8dp"
                        adaptive_width: True
                        pos_hint: {"center_x": 0.5}
                        MDTextField:
                            id: client_city_input
                            hint_text: "Kota Klien (opsional, utk geofence layer 3)"
                            multiline: False
                            size_hint_x: 0.55
                        MDRaisedButton:
                            text: "Gunakan Kota"
                            theme_text_color: "Custom"
                            text_color: 0.9, 0.9, 1, 1
                            on_release: app.use_client_city()

                    # ---------- Actions ----------
                MDBoxLayout:
                    size_hint_y: None
                    height: "52dp"
                    spacing: "8dp"
                    adaptive_width: True
                    pos_hint: {"center_x": 0.5}
                    MDRaisedButton:
                        id: start_btn
                        text: "Start Kamera"
                        on_release: app.start_camera()
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
'''

# --------------------------------------------------------------------------
# Camera preview widget that renders numpy BGR frames into a Kivy texture
# --------------------------------------------------------------------------
class CameraPreview(Image):
    """Displays a numpy BGR frame as a Kivy texture (shared desktop/mobile)."""

    def show_frame(self, frame_bgr):
        if frame_bgr is None:
            return
        h, w = frame_bgr.shape[:2]
        buf = np.flip(frame_bgr, axis=0)  # vertical flip for natural preview
        buf = buf.tobytes()
        texture = Texture.create(size=(w, h), colorfmt="bgr")
        texture.blit_buffer(buf, colorfmt="bgr", bufferfmt="ubyte")
        texture.flip_vertical()
        self.texture = texture


# --------------------------------------------------------------------------
# App
# --------------------------------------------------------------------------
class AntiTimpaApp(MDApp):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.core = QrisScannerCore(blur_threshold=100.0, fifo_size=5)
        self.cap = None
        self.is_running = False
        self.sim_count = 0
        self.notify_ev = None
        self.client_city = None

    def use_client_city(self):
        """Set kota klien dari input (desktop tak punya GPS)."""
        try:
            val = self.root.ids.get("client_city_input").text.strip()
        except Exception:
            val = ""
        if val:
            self.client_city = val.upper()
            self._set_status(f"Kota klien di-set: {self.client_city} (Layer 3 aktif).")
        else:
            self.client_city = None
            self._set_status("Kota klien dikosongkan — Layer 3 dilewati.")

    def build(self):
        self.theme_cls.primary_palette = "Blue"
        self.theme_cls.theme_style = "Dark"
        return Builder.load_string(KV)

    # ----------------------------------------------------------------
    # Camera lifecycle
    # ----------------------------------------------------------------
    def start_camera(self):
        if self.is_running:
            return
        self.is_running = True

        if platform == "desktop":
            # Desktop can feed raw numpy frames straight from the OpenCV webcam.
            self._open_opencv_camera()
        else:
            # On Android/iOS, Kivy's native camera can't return raw numpy frames
            # for OpenCV analysis reliably; use a synthetic demo generator so the
            # full pipeline runs on-device without a hardware feed. Real raw-frame
            # access needs a platform camera bridge (see README roadmap).
            self._start_synthetic()

        self.notify_ev = Clock.schedule_interval(self._tick, 1.0 / 30.0)
        self._set_status("Kamera aktif (lokal).")

    def _open_opencv_camera(self):
        src = os.environ.get("CAMERA_SOURCE", "0")
        try:
            idx = int(src)
            self.cap = cv2.VideoCapture(idx)
        except ValueError:
            self.cap = cv2.VideoCapture(src)
        if self.cap is None or not self.cap.isOpened():
            self._set_status("Kamera tidak terdeteksi. Gunakan mode demo (sintetik).")
            self._start_synthetic()

    def _start_synthetic(self):
        # Keep cap=None; synthetic feed generated in _tick
        self.cap = None
        self.sim_count = 0
        self._set_status("Mode simulasi (demo).")

    def stop_work(self):
        self.is_running = False
        if self.notify_ev:
            self.notify_ev.cancel()
            self.notify_ev = None
        if self.cap is not None:
            try:
                self.cap.release()
            except Exception:
                pass
            self.cap = None
        self._set_status("Dihentikan.")

    # ----------------------------------------------------------------
    # Synthetic frame generator (mirrors live_scanner.SyntheticFrameGenerator)
    # ----------------------------------------------------------------
    def _synthetic_frame(self):
        frame = np.full((480, 640, 3), 240, dtype=np.uint8)
        qx, qy, qw, qh = 200, 140, 240, 240
        cv2.rectangle(frame, (qx - 20, qy - 20), (qx + qw + 20, qy + qh + 20), (255, 255, 255), -1)
        cv2.rectangle(frame, (qx, qy), (qx + qw, qy + qh), (0, 0, 0), 4)
        cv2.rectangle(frame, (qx + 10, qy + 10), (qx + 60, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + qw - 60, qy + 10), (qx + qw - 10, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + 10, qy + qh - 60), (qx + 60, qy + qh - 10), (0, 0, 0), -1)
        if (self.sim_count // 30) % 2 == 1:
            cv2.rectangle(frame, (qx - 15, qy - 15), (qx + qw + 15, qy + qh + 15), (50, 50, 50), 3)
        return frame

    def _tick_synthetic_payload(self):
        mode = (self.sim_count // 30) % 4
        if mode == 0:
            return "00020101021126330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG MAKMUR6007JAKARTA63041B52"
        elif mode == 1:
            return "00020101021126330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG HACKED6007JAKARTA63041B52"
        elif mode == 2:
            return "00020101021126330010A0000006020115ID10200000000015204866153033605802ID5919TOKO CHARITY BERKAH6007JAKARTA6304278F"
        else:
            return "00020101021226330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG MAKMUR6007JAKARTA6304679C"

    # ----------------------------------------------------------------
    # Main per-frame loop
    # ----------------------------------------------------------------
    def _tick(self, _dt):
        if not self.is_running:
            return

        # Acquire a frame
        if self.cap is not None:
            ok, frame = self.cap.read()
            if not ok or frame is None:
                self._set_status("Feed berakhir. Beralih ke mode simulasi.")
                self.cap = None
                self.sim_count = 0
                frame = self._synthetic_frame()
                force_bbox = (200, 140, 240, 240)
                force_raw = self._tick_synthetic_payload()
            else:
                force_bbox = None
                force_raw = None
        else:
            # Synthetic mode
            self.sim_count += 1
            frame = self._synthetic_frame()
            force_bbox = (200, 140, 240, 240)
            force_raw = self._tick_synthetic_payload()

        snap = self.core.process_frame(
            frame,
            optical_type="physical_camera_scan",
            force_bbox=force_bbox,
            force_raw=force_raw,
            client_city=self.client_city,
        )

        # Render preview + HUD
        hud = self._draw_hud(frame, snap)
        self._show_preview(hud)
        self._update_results(snap, feed_mode="simulasi" if self.cap is None else "kamera")

    # ----------------------------------------------------------------
    # HUD rendering (mirrors live_scanner.draw_hud, adapted to UI text)
    # ----------------------------------------------------------------
    def _draw_hud(self, frame, snap):
        display = frame.copy()
        if snap['qr_bbox'] is not None and not snap['is_blurry']:
            x, y, bw, bh = snap['qr_bbox']
            box_color = self._risk_color(snap['combined_score'], snap['l2']['crc_valid'])
            cv2.rectangle(display, (x, y), (x + bw, y + bh), box_color, 3)
            try:
                x_exp, y_exp, w_exp, h_exp = expand_bounding_box(snap['qr_bbox'], display.shape[1], display.shape[0], padding_percent=0.10)
                cv2.rectangle(display, (x_exp, y_exp), (x_exp + w_exp, y_exp + h_exp), box_color, 1)
            except Exception:
                pass
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
        if preview is not None:
            preview.show_frame(frame_bgr)

    # ----------------------------------------------------------------
    # UI updates
    # ----------------------------------------------------------------
    def _update_results(self, snap, feed_mode="kamera"):
        ids = self.root.ids
        l1 = snap['l1']
        l2 = snap['l2']
        l3 = snap.get('l3', {})

        # Risk card
        risk_lbl = ids.get("risk_level_label")
        if risk_lbl is not None:
            risk_lbl.text = snap['combined_risk_level']
        sc = ids.get("risk_score_label")
        if sc is not None:
            sc.text = f"Score Gabungan: {snap['combined_score']:.3f}"

        warnings = l2.get('warnings', []) + l3.get('warnings', [])
        warns = ids.get("risk_warnings_label")
        if warns is not None:
            warns.text = "Warnings: " + ("; ".join(warnings) if warnings else "Tidak ada")

        l1_t = ids.get("l1_text")
        if l1_t is not None:
            l1_t.text = (f"Edge Density: {l1.get('spatial_edge_density', 0):.3f}  |  "
                         f"Glare Var: {l1.get('temporal_glare_var', 0):.5f}  |  "
                         f"Skor: {l1.get('l1_score', 0):.2f}")

        l2_t = ids.get("l2_text")
        if l2_t is not None:
            l2_t.text = (f"CRC: {'VALID' if l2.get('crc_valid', False) else 'GAGAL'}  |  "
                         f"Merchant: {l2.get('merchant_name') or 'N/A'}  |  "
                         f"MCC: {l2.get('mcc') or 'N/A'}  |  "
                         f"Kota: {l2.get('merchant_city') or 'N/A'}")

        mode_t = ids.get("l2_mode_text")
        if mode_t is not None:
            mode_t.text = f"Mode: {l2.get('initiation_mode') or 'N/A'}  |  Payload: {l2.get('parsed_tlv', {}).get('00', 'N/A')}"

        l3_t = ids.get("l3_text")
        if l3_t is not None:
            l3_t.text = (f"Kota Klien: {l3.get('client_city') or 'N/A'}  |  "
                         f"Kota Merchant: {l3.get('merchant_city') or 'N/A'}  |  "
                         f"Skor: {l3.get('l3_score', 0):.2f}")

        l3_status = ids.get("l3_status_text")
        if l3_status is not None:
            l3_status.text = f"Status: {l3.get('risk_level', 'N/A')}"

        status = ids.get("status_text")
        if status is not None:
            status.text = f"Feed: {feed_mode}  |  BlurVar: {snap['blur_var']:.1f}  |  Status: {'BLUR' if snap['is_blurry'] else 'CLEAR'}"

        banner = ids.get("banner_label")
        if banner is not None:
            banner.text = f"{snap['combined_risk_level']} | Score {snap['combined_score']:.2f}"

    def _set_status(self, msg):
        status = self.root.ids.get("status_text") if hasattr(self, "root") else None
        if status is not None:
            status.text = f"Status: {msg}"

    # ----------------------------------------------------------------
    # Import image (one-shot validation - fully local)
    # ----------------------------------------------------------------
    def import_image(self):
        try:
            from plyer import filechooser
        except Exception:
            Snackbar(text="File picker tidak tersedia di platform ini.").open()
            return

        if platform == "desktop":
            path = filechooser.open_file(filters=["*.png", "*.jpg", "*.jpeg"])
            if path:
                self._analyze_imported_file(path[0])
        else:
            Snackbar(text="Pilih file QRIS dari galeri (lokal).").open()
            filechooser.open_file(on_selection=self._on_file_selected)

    def _on_file_selected(self, selection):
        if selection:
            self._analyze_imported_file(selection[0])

    def _analyze_imported_file(self, path):
        try:
            frame = cv2.imread(path)
            if frame is None:
                Snackbar(text="Gagal membaca gambar.").open()
                return
            result = QrisScannerCore.analyze_image(frame, optical_type="imported_image")
            if not result['ok']:
                Snackbar(text=result.get('error', "Tidak ada QR terdeteksi.")).open()
                return
            # Show result snapshot on the results panel
            self._update_results(result['snapshot'], feed_mode="import")
            self._show_preview(self._draw_hud(frame, result['snapshot']))
            Snackbar(text="Analisis gambar selesai (lokal).").open()
        except Exception as e:
            Snackbar(text=f"Error: {e}").open()

    def on_stop(self):
        self.stop_work()


def main():
    AntiTimpaApp().run()


if __name__ == "__main__":
    main()
