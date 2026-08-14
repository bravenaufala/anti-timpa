"""
Anti Timpa QRIS - Core Scanner Engine (platform-agnostic)

This module is the single analysis core used by the KivyMD app on ALL platforms
(desktop + Android + iOS). It runs entirely ON-DEVICE: no network calls, no
cloud upload. Raw frames are analyzed locally by Layer 1 (optical tampering)
and Layer 2 (EMVCo payload validation).

It reuses the existing untouched analysis modules:
    layer1_optical.py  -> process_layer1_edge (physical tampering detection)
    layer2_emvco.py    -> process_layer2_tlv  (payload / CRC / risk rules)

Design notes
------------
* `analyze_frame()` takes a BGR numpy frame (same format the desktop OpenCV
  camera produces). On mobile, the Kivy UI converts its camera texture to a
  numpy BGR frame and feeds it here — keeping all analysis identical everywhere.
* A small sliding FIFO of CLEAR frames is kept internally for Layer 1 temporal
  glare analysis, mirroring the original live_scanner.py.
* Everything is deterministic and local; results are returned as plain dicts
  so the UI (Kivy or anything else) can render them.
"""

from collections import deque
from typing import Any, Dict, Optional, Tuple, Union

import cv2
import numpy as np

from layer1_optical import process_layer1_edge
from layer2_emvco import process_layer2_tlv
from layer3_geofence import process_layer3_geofence


class QrisScannerCore:
    """
    On-device QRIS security scanner engine.

    Usage
    -----
        core = QrisScannerCore(blur_threshold=100.0, fifo_size=5)
        result = core.process_frame(frame_bgr, optical_type="physical_camera_scan")
    """

    def __init__(self, blur_threshold: float = 100.0, fifo_size: int = 5):
        self.blur_threshold = blur_threshold
        self.fifo_size = fifo_size
        self.fifo_queue: deque = deque(maxlen=fifo_size)
        self.qr_detector = cv2.QRCodeDetector()

        # Retained state mirroring live_scanner.py
        self.last_raw_qris_str: str = ""
        # Menghitung frame berturut-turut tanpa QR, utk reset (agar scan bisa
        # "take berkali-kali": risiko refresh setiap kali menunjuk QR baru).
        self.no_qr_frames: int = 0
        self.reset_after: int = 4

        # Default result (no QR yet)
        self._reset_metrics()

    def _reset_metrics(self) -> None:
        self.l1_metrics: Dict[str, Any] = {
            'l1_score': 0.0,
            'spatial_edge_density': 0.0,
            'temporal_glare_var': 0.0,
            'risk_level': 'NO QR',
        }
        self.l2_metrics: Dict[str, Any] = {
            'l2_score': 0.0,
            'crc_valid': True,
            'initiation_mode': '',
            'mcc': '',
            'merchant_name': '',
            'merchant_city': '',
            'parsed_tlv': {},
            'warnings': [],
        }
        self.l3_metrics: Dict[str, Any] = {
            'l3_score': 0.0,
            'risk_level': 'NO QR',
            'warnings': [],
            'client_city': None,
            'merchant_city': None
        }
        self.combined_score: float = 0.0
        self.combined_risk_level: str = 'NO QR'
        self.is_blurry: bool = False
        self.blur_var: float = 0.0
        self.qr_bbox: Optional[Tuple[int, int, int, int]] = None

    @staticmethod
    def _compute_blur(frame: np.ndarray) -> float:
        """Laplacian variance blur score. Lower = more blurry."""
        gray = frame if len(frame.shape) == 2 else cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
        return float(cv2.Laplacian(gray, cv2.CV_64F).var())

    def detect_qr(self, frame: np.ndarray, aggressive: bool = False) -> Tuple[Optional[Tuple[int, int, int, int]], Optional[np.ndarray], str]:
        """
        Detect QR codes. Returns (qr_bbox, poly_points, raw_qris_str).

        * aggressive=False (jalur kamera live, harus RINGAN): hanya satu panggilan
          detectAndDecodeMulti. Tidak ada fallback yang mahal.
        * aggressive=True (import gambar galeri, one-shot): mencoba beberapa
          strategi, termasuk deteksi lokasi via .detect() lalu crop+upscale,
          karena cv2 hanya bisa meng-decode QR kecil bila crop diperbesar.
        """
        raw_qris_str = ""
        qr_bbox = None
        poly_points = None

        # helper untuk mengekstrak hasil detection
        def _extract(points, decoded_info=None):
            nonlocal qr_bbox, poly_points, raw_qris_str
            if points is None:
                return None
            pts = points[0] if points.ndim == 3 else points
            if pts is None or len(pts) < 4:
                return None
            pts_int = pts.astype(np.int32)
            x, y, w, h = cv2.boundingRect(pts_int)
            if w <= 10 or h <= 10:
                return None
            qr_bbox = (x, y, w, h)
            poly_points = pts_int
            if isinstance(decoded_info, (list, tuple)) and len(decoded_info) > 0:
                raw_qris_str = str(decoded_info[0])
            elif isinstance(decoded_info, str):
                raw_qris_str = decoded_info
            return qr_bbox

        # 1) jalur ringan / live: satu panggilan saja
        try:
            retval, info, points, _ = self.qr_detector.detectAndDecodeMulti(frame)
            if retval and _extract(points, info):
                return qr_bbox, poly_points, raw_qris_str
        except Exception:
            pass

        if not aggressive:
            return qr_bbox, poly_points, raw_qris_str

        # ---- aggressive (import gambar): pakai pyzbar dulu (paling andal) ----
        # pyzbar/zbar sangat baik menemukan QR kecil di gambar besar dan miring.
        try:
            from pyzbar import pyzbar as _pyzbar
            from PIL import Image as _PIL
            pil_img = _PIL.fromarray(frame)
            decoded = _pyzbar.decode(pil_img)
            if decoded:
                b = decoded[0]
                raw_str = (b.data or b"").decode("utf-8", "replace")
                left, top, ww, hh = b.rect
                if ww > 5 and hh > 5:
                    pts_py = np.array([[left, top],
                                       [left + ww, top],
                                       [left + ww, top + hh],
                                       [left, top + hh]], dtype=np.float32)
                    pts_py = pts_py.reshape(1, 4, 2)
                    if _extract(pts_py, raw_str):
                        return qr_bbox, poly_points, raw_qris_str
        except Exception:
            pass

        # siapkan versi praproses
        try:
            if len(frame.shape) == 3:
                gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            else:
                gray = frame.copy()
        except Exception:
            gray = None

        candidates = []
        if gray is not None:
            candidates.append((gray, "gray"))
            try:
                # perbaikan kontras lokal (CLAHE) bisa menolong QR redup
                clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))
                candidates.append((clahe.apply(gray), "clahe"))
            except Exception:
                pass

        # 2) coba detectAndDecode pada tiap versi, plus beberapa skala
        for img, name in candidates:
            h_img, w_img = img.shape[:2]
            scales = [1.0]
            if w_img < 600:
                scales.append(2.0)   # perbesar jika resolusi kecil
            if w_img > 2000:
                scales.append(0.5)   # perkecil jika resolusi sangat besar
            if aggressive and w_img <= 2500:
                # Saat import gambar, coba juga upscale 2x utk memudahkan
                # deteksi QR yang kecil di dalam foto galeri yang besar.
                scales.append(2.0)
            for s in scales:
                try:
                    work = img
                    if s != 1.0:
                        work = cv2.resize(img, (int(w_img * s), int(h_img * s)),
                                          interpolation=cv2.INTER_CUBIC)
                    raw_str, pts, _ = self.qr_detector.detectAndDecode(work)
                    if pts is not None and len(pts) >= 4:
                        # bila digubah skala, peta koordinat kembali ke skala asli
                        if s != 1.0:
                            pts = pts / s
                        if _extract(pts, raw_str):
                            return qr_bbox, poly_points, raw_qris_str
                except Exception:
                    continue

        # 3) aggressive: deteksi lokasi QR lewat .detect(), lalu crop area & upscale
        #    sebelum decode. QR kecil di dalam foto besar kerap gagal di-decode
        #    langsung, tapi crop yang diperbesar -> ~300px berhasil (terbukti).
        if aggressive and gray is not None:
            h_img, w_img = gray.shape[:2]
            try:
                found, pts = self.qr_detector.detect(gray)
                if found and pts is not None:
                    for group in (pts if pts.ndim == 3 else [pts]):
                        try:
                            if group is None or len(group) < 4:
                                continue
                            b = cv2.boundingRect(group.astype(np.int32))
                            bx, by, bw, bh = b
                            pad = max(bw, bh) // 3
                            x0 = max(0, bx - pad)
                            y0 = max(0, by - pad)
                            x1 = min(w_img, bx + bw + pad)
                            y1 = min(h_img, by + bh + pad)
                            crop = gray[y0:y1, x0:x1]
                            if crop.size == 0:
                                continue
                            # upscale supaya sisi terpanjang >= ~300px (agar ter-decode)
                            scale = max(1.0, 300.0 / max(crop.shape))
                            if scale > 1.0:
                                crop = cv2.resize(
                                    crop,
                                    (int(crop.shape[1] * scale), int(crop.shape[0] * scale)),
                                    interpolation=cv2.INTER_CUBIC,
                                )
                            raw_str, dpts, _ = self.qr_detector.detectAndDecode(crop)
                            if raw_str and dpts is not None and len(dpts) >= 4:
                                pts_abs = dpts / scale if scale > 1.0 else dpts
                                pts_abs[:, :, 0] += x0
                                pts_abs[:, :, 1] += y0
                                if _extract(pts_abs, raw_str):
                                    return qr_bbox, poly_points, raw_qris_str
                        except Exception:
                            continue
            except Exception:
                pass

        return qr_bbox, poly_points, raw_qris_str

    def process_frame(
        self,
        frame: np.ndarray,
        optical_type: str = "physical_camera_scan",
        force_bbox: Optional[Tuple[int, int, int, int]] = None,
        force_raw: Optional[str] = None,
        client_city: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Process a single BGR frame through the full dual-layer pipeline.

        Parameters mirror the desktop live scanner:
        * force_bbox: optionally override QR detection (used by synthetic/demo mode).
        * force_raw:  optionally override the decoded QR string (demo / imported image).

        Returns a snapshot dict of the current metrics for UI rendering.
        """
        if frame is None:
            return self.snapshot()

        # --- Blur gatekeeper ---
        self.blur_var = self._compute_blur(frame)
        self.is_blurry = self.blur_var < self.blur_threshold

        # --- QR detection ---
        if force_bbox is not None:
            self.qr_bbox = force_bbox
            raw_qris_str = force_raw or ""
        else:
            self.qr_bbox, _, raw_qris_str = self.detect_qr(frame)

        # Retain the decoded QR string alongside score calculation
        if raw_qris_str:
            self.last_raw_qris_str = raw_qris_str

        # --- Queue management + layers ---
        if self.qr_bbox is not None and not self.is_blurry:
            self.fifo_queue.append(frame.copy())

            # Layer 1: optical tampering
            self.l1_metrics = process_layer1_edge(list(self.fifo_queue), self.qr_bbox)

            # Layer 2: EMVCo payload rules
            self.l2_metrics = process_layer2_tlv(
                self.last_raw_qris_str,
                scan_context={"optical_type": optical_type},
            )

            # Layer 3: Geofence (City match)
            self.l3_metrics = process_layer3_geofence(
                client_city,
                self.l2_metrics.get('merchant_city')
            )

            l1_score = self.l1_metrics.get('l1_score', 0.0)
            l2_score = self.l2_metrics.get('l2_score', 0.0)
            l3_score = self.l3_metrics.get('l3_score', 0.0)
            crc_valid = self.l2_metrics.get('crc_valid', True)

            # Hard-veto when CRC invalid
            if not crc_valid:
                self.combined_score = 1.0
            else:
                self.combined_score = max(l1_score, l2_score, l3_score)

            if self.combined_score < 0.35 and crc_valid:
                self.combined_risk_level = 'LOW RISK'
            elif self.combined_score <= 0.70 and crc_valid:
                self.combined_risk_level = 'CAUTION'
            else:
                self.combined_risk_level = 'HIGH RISK'
            # Ada QR -> reset penghitung kegagalan.
            self.no_qr_frames = 0
        elif self.is_blurry:
            # Clear queue while blurry to preserve frame quality
            self.fifo_queue.clear()

        # --- Reset bila QR tidak terlihat beberapa frame berturut-turut ---
        # Ini membuat scan "take berkali-kali": begitu kamera tidak lagi melihat
        # QR, hasil kembali idel (NO QR) sehingga menunjuk QR lain akan
        # memperbarui skor/risiko dengan benar. Kecuali sedang gaya demo
        # (force_bbox/force_raw) yang dipegang terus.
        if raw_qris_str:
            self.no_qr_frames = 0
        elif force_bbox is None:
            self.no_qr_frames += 1
            if self.no_qr_frames >= self.reset_after and (
                    self.combined_risk_level != 'NO QR'):
                self.last_raw_qris_str = ""
                self.fifo_queue.clear()
                self._reset_metrics()

        return self.snapshot()

    def snapshot(self) -> Dict[str, Any]:
        """Return the latest metrics snapshot for UI rendering."""
        return {
            'l1': dict(self.l1_metrics),
            'l2': dict(self.l2_metrics),
            'l3': dict(self.l3_metrics),
            'combined_score': float(self.combined_score),
            'combined_risk_level': self.combined_risk_level,
            'is_blurry': bool(self.is_blurry),
            'blur_var': float(self.blur_var),
            'qr_bbox': self.qr_bbox,
            'raw_qris_str': self.last_raw_qris_str,
        }

    @staticmethod
    def analyze_image(
        frame_bgr: np.ndarray,
        blur_threshold: float = 100.0,
        optical_type: str = "imported_image",
    ) -> Dict[str, Any]:
        """
        One-shot analysis of a single image (no live stream), used by the
        import-from-gallery feature. Calculates both layers without the timestream
        glare component (single frame), so glare variance is 0.
        """
        bbox, _, raw_str = QrisScannerCore().detect_qr(frame_bgr, aggressive=True)
        if bbox is None:
            return {'ok': False, 'error': 'No QR code detected in image', 'snapshot': None}

        core = QrisScannerCore(blur_threshold=blur_threshold)
        pass_bbox = (bbox[0], bbox[1], bbox[2], bbox[3])
        snap = core.process_frame(frame_bgr, optical_type=optical_type, force_bbox=pass_bbox, force_raw=raw_str)
        return {'ok': True, 'bbox': bbox, 'raw': raw_str, 'snapshot': snap}
