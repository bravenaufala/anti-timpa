"""
Anti Timpa QRIS Fintech SDK - Layer 1 & 2: Live Camera Scanner
Module: live_scanner.py

Captures live camera feed, detects QRIS codes, evaluates frame clarity (Laplacian blur),
manages a 5-frame FIFO queue of clear frames, computes Layer 1 physical tampering risk score,
retains raw QR string for Layer 2 pipeline, processes Layer 2 EMVCo TLV parsing and risk scoring,
and renders an upgraded diagnostic HUD.
"""

import os
import sys
import time
import argparse
from collections import deque
import cv2
import numpy as np
from typing import Optional, Tuple, List, Dict, Any, Union

from layer1_optical import process_layer1_edge, expand_bounding_box
from layer2_emvco import process_layer2_tlv


def parse_args():
    parser = argparse.ArgumentParser(description="Anti Timpa QRIS Dual-Layer Live Optical Scanner")
    parser.add_argument("--cam", "--source", dest="cam", type=str, default=None, 
                        help="Camera index (e.g. 0, 2) or video file path. Overrides CAMERA_SOURCE env var.")
    parser.add_argument("--blur-threshold", type=float, default=100.0,
                        help="Laplacian variance threshold for frame clarity gatekeeper")
    parser.add_argument("--fifo-size", type=int, default=5,
                        help="Maximum size of clear frame FIFO queue")
    parser.add_argument("--simulated", action="store_true",
                        help="Run with synthetic frame generator for testing without webcam")
    return parser.parse_args()


class SyntheticFrameGenerator:
    """Generates synthetic QR code frames for headless testing or demo modes."""
    def __init__(self, width: int = 640, height: int = 480):
        self.width = width
        self.height = height
        self.frame_count = 0

    def read(self) -> Tuple[bool, np.ndarray]:
        self.frame_count += 1
        frame = np.full((self.height, self.width, 3), 240, dtype=np.uint8)

        # Draw a synthetic QR code in center
        qx, qy, qw, qh = 200, 140, 240, 240
        # Draw background paper (white)
        cv2.rectangle(frame, (qx - 20, qy - 20), (qx + qw + 20, qy + qh + 20), (255, 255, 255), -1)

        # Draw QR alignment patterns & grid
        cv2.rectangle(frame, (qx, qy), (qx + qw, qy + qh), (0, 0, 0), 4)
        # Outer locator boxes
        cv2.rectangle(frame, (qx + 10, qy + 10), (qx + 60, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + qw - 60, qy + 10), (qx + qw - 10, qy + 60), (0, 0, 0), -1)
        cv2.rectangle(frame, (qx + 10, qy + qh - 60), (qx + 60, qy + qh - 10), (0, 0, 0), -1)

        # Simulate periodic sticker overlay line or glare shift
        if (self.frame_count // 30) % 2 == 1:
            # Simulate high-frequency sticker cut-out boundary in quiet zone margin
            cv2.rectangle(frame, (qx - 15, qy - 15), (qx + qw + 15, qy + qh + 15), (50, 50, 50), 3)

        # Add minor dynamic motion blur or glare periodically
        if self.frame_count % 15 == 0:
            frame = cv2.GaussianBlur(frame, (11, 11), 0)

        time.sleep(0.03)  # ~30 FPS delay
        return True, frame


def resolve_camera_source(preferred_source: Optional[Union[str, int]] = None) -> Union[int, str]:
    """
    Resolves the preferred camera source priority:
    1. preferred_source argument (from CLI --cam / --source)
    2. CAMERA_SOURCE environment variable
    3. Default integer 0
    Converts string digits to integer IDs (e.g., "2" -> 2).
    """
    source_val = preferred_source
    if source_val is None or str(source_val).strip() == "":
        source_val = os.environ.get("CAMERA_SOURCE", "0")

    source_str = str(source_val).strip()
    if source_str.isdigit():
        return int(source_str)
    return source_str


def get_universal_camera_capture(preferred_source: Optional[Union[str, int]] = None) -> Tuple[Any, bool]:
    """
    Cross-platform, universal camera stream initializer supporting auto-discovery and graceful fallbacks.
    Returns (cap_object, is_simulated_flag).
    """
    source = resolve_camera_source(preferred_source)

    # Handle video file paths or stream URLs (non-digit strings)
    if isinstance(source, str):
        print(f"[INFO] Connecting to video file / stream source: '{source}'")
        cap = cv2.VideoCapture(source, cv2.CAP_ANY)
        if cap.isOpened():
            print(f"[INFO] Successfully opened stream source: '{source}'")
            return cap, False
        else:
            print(f"[WARNING] Unable to open video source '{source}'. Falling back to synthetic generator mode.")
            return SyntheticFrameGenerator(), True

    # Handle hardware camera integer ID
    preferred_idx = source
    print(f"[INFO] Attempting to open preferred camera ID: {preferred_idx}")
    cap = cv2.VideoCapture(preferred_idx, cv2.CAP_ANY)
    if cap.isOpened():
        print(f"[INFO] Successfully connected to camera at index {preferred_idx}")
        return cap, False

    # Auto-Discovery Fallback Loop across camera indices [0, 1, 2, 3, 4]
    print(f"[INFO] Preferred camera {preferred_idx} not found.")
    print("[INFO] Initiating auto-discovery camera scan across indices [0, 1, 2, 3, 4]...")

    fallback_indices = [idx for idx in [0, 1, 2, 3, 4] if idx != preferred_idx]
    for idx in fallback_indices:
        try_cap = cv2.VideoCapture(idx, cv2.CAP_ANY)
        if try_cap.isOpened():
            print(f"[INFO] Preferred camera {preferred_idx} not found. Auto-detected working camera at index {idx}.")
            return try_cap, False
        else:
            try_cap.release()

    # Diagnostic warning if no camera hardware is available
    print("[WARNING] No working hardware camera detected across indices [0, 1, 2, 3, 4].")
    print("[HINT] Consider running with --simulated flag for virtual test feed.")
    print("[INFO] Defaulting to synthetic frame generator for demonstration...")
    return SyntheticFrameGenerator(), True


def detect_qr_code(
    detector: cv2.QRCodeDetector, 
    frame: np.ndarray
) -> Tuple[Optional[Tuple[int, int, int, int]], Optional[np.ndarray], str]:
    """
    Detects QR codes using OpenCV's QRCodeDetector.
    Safely converts QR polygon points into bounding box tuple (x, y, w, h).
    Handles None or empty detections cleanly without crashing.
    Returns: (qr_bbox, poly_points, raw_qris_str)
    """
    raw_qris_str = ""
    qr_bbox = None
    poly_points = None

    try:
        retval, decoded_info, points, _ = detector.detectAndDecodeMulti(frame)
        if retval and points is not None and len(points) > 0:
            pts = points[0]
            if pts is not None and len(pts) >= 4:
                pts_int = pts.astype(np.int32)
                x, y, w, h = cv2.boundingRect(pts_int)
                if w > 10 and h > 10:
                    qr_bbox = (x, y, w, h)
                    poly_points = pts_int
                    if isinstance(decoded_info, (list, tuple)) and len(decoded_info) > 0:
                        raw_qris_str = str(decoded_info[0])
                    elif isinstance(decoded_info, str):
                        raw_qris_str = decoded_info
    except Exception:
        try:
            raw_str, points, _ = detector.detectAndDecode(frame)
            if points is not None and len(points) >= 4:
                pts_int = points[0].astype(np.int32) if len(points.shape) == 3 else points.astype(np.int32)
                x, y, w, h = cv2.boundingRect(pts_int)
                if w > 10 and h > 10:
                    qr_bbox = (x, y, w, h)
                    poly_points = pts_int
                    raw_qris_str = str(raw_str) if raw_str else ""
        except Exception:
            pass

    return qr_bbox, poly_points, raw_qris_str


def draw_hud(
    frame: np.ndarray,
    fps: float,
    is_blurry: bool,
    blur_var: float,
    qr_bbox: Optional[Tuple[int, int, int, int]],
    l1_metrics: Dict[str, Any],
    l2_metrics: Dict[str, Any],
    combined_score: float,
    combined_risk_level: str,
    raw_qris_str: str
) -> np.ndarray:
    """
    Renders diagnostic HUD overlay with metrics, frame status, QR string output,
    and color-coded target bounding box.
    """
    display_frame = frame.copy()
    h, w = display_frame.shape[:2]

    # Overlay Semi-Transparent Top Banner (extended to 125px for 4 lines of diagnostics)
    overlay = display_frame.copy()
    banner_height = 125
    cv2.rectangle(overlay, (0, 0), (w, banner_height), (20, 20, 20), -1)
    cv2.addWeighted(overlay, 0.75, display_frame, 0.25, 0, display_frame)

    # 1. SDK Title, FPS, Blur status, Blur Var
    clarity_text = "BLURRY - HOLD CAMERA STILL" if is_blurry else "CLEAR"
    clarity_color = (0, 165, 255) if is_blurry else (0, 255, 0)
    
    cv2.putText(display_frame, f"ANTI TIMPA DUAL-LAYER SCANNER | FPS: {fps:.1f} | STATUS: {clarity_text} (Blur Var: {blur_var:.1f})", 
                (15, 25), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (255, 255, 255), 1)

    # 2. L1 Optical Metrics (Edge Density, Glare Var) & L1 Score
    l1_score = l1_metrics.get('l1_score', 0.0)
    edge_density = l1_metrics.get('spatial_edge_density', 0.0)
    glare_var = l1_metrics.get('temporal_glare_var', 0.0)
    l1_text = f"L1 OPTICAL -> Score: {l1_score:.2f} | Edge Density: {edge_density:.3f} | Glare Var: {glare_var:.5f}"
    cv2.putText(display_frame, l1_text, (15, 52), cv2.FONT_HERSHEY_SIMPLEX, 0.45, (220, 220, 220), 1)

    # 3. L2 Payload Metrics (CRC Valid, Initiation Mode, MCC, Merchant Name, City) & L2 Score
    crc_valid_str = "YES" if l2_metrics.get("crc_valid", True) else "NO"
    init_mode = l2_metrics.get("initiation_mode") or "N/A"
    mcc = l2_metrics.get("mcc") or "N/A"
    mname = l2_metrics.get("merchant_name") or "N/A"
    mcity = l2_metrics.get("merchant_city") or "N/A"
    l2_score = l2_metrics.get("l2_score", 0.0)
    l2_text = f"L2 PAYLOAD -> Score: {l2_score:.2f} | CRC Valid: {crc_valid_str} | Init Mode: {init_mode} | MCC: {mcc} | Name: {mname} | City: {mcity}"
    cv2.putText(display_frame, l2_text, (15, 79), cv2.FONT_HERSHEY_SIMPLEX, 0.45, (220, 220, 220), 1)

    # 4. Combined Score & Active Warnings
    warnings = l2_metrics.get("warnings", [])
    warnings_text = ", ".join(warnings) if warnings else "NONE"
    l4_text = f"COMBINED SCORE: {combined_score:.3f} [{combined_risk_level}] | Warnings: {warnings_text}"
    
    if combined_risk_level == "LOW RISK":
        l4_color = (0, 255, 0)
    elif combined_risk_level == "CAUTION":
        l4_color = (0, 255, 255)
    else:
        l4_color = (0, 0, 255)
        
    cv2.putText(display_frame, l4_text, (15, 106), cv2.FONT_HERSHEY_SIMPLEX, 0.45, l4_color, 1)

    # Prominent Warning Prompt if Blurry
    if is_blurry:
        cv2.rectangle(display_frame, (w // 2 - 220, h // 2 - 25), (w // 2 + 220, h // 2 + 25), (0, 0, 0), -1)
        cv2.rectangle(display_frame, (w // 2 - 220, h // 2 - 25), (w // 2 + 220, h // 2 + 25), (0, 165, 255), 2)
        cv2.putText(display_frame, "HOLD CAMERA STILL", (w // 2 - 180, h // 2 + 8), 
                    cv2.FONT_HERSHEY_SIMPLEX, 0.9, (0, 165, 255), 3)

    # Color-Coded QR Bounding Box & Risk Badge
    if qr_bbox is not None and not is_blurry:
        x, y, bw, bh = qr_bbox
        
        # Color coding: Green (< 0.35), Yellow (0.35-0.70), Red (> 0.70 or CRC Invalid)
        crc_valid = l2_metrics.get("crc_valid", True)
        if combined_score < 0.35 and crc_valid:
            box_color = (0, 255, 0)      # Green
        elif combined_score <= 0.70 and crc_valid:
            box_color = (0, 255, 255)    # Yellow
        else:
            box_color = (0, 0, 255)      # Red

        # Draw main QR box
        cv2.rectangle(display_frame, (x, y), (x + bw, y + bh), box_color, 3)

        # Draw 20% expanded Quiet Zone outer margin box
        x_exp, y_exp, w_exp, h_exp = expand_bounding_box(qr_bbox, w, h, padding_percent=0.10)
        cv2.rectangle(display_frame, (x_exp, y_exp), (x_exp + w_exp, y_exp + h_exp), box_color, 1)

        # Draw Risk Score Badge on target box
        badge_text = f"RISK: {combined_score:.2f} [{combined_risk_level}]"
        cv2.rectangle(display_frame, (x, max(0, y - 30)), (x + len(badge_text) * 11, max(0, y)), box_color, -1)
        cv2.putText(display_frame, badge_text, (x + 5, max(15, y - 8)), 
                    cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 0, 0), 2)

    return display_frame


def run_live_scanner(args):
    """Main camera scanner loop."""
    print("=" * 60)
    print("ANTI TIMPA QRIS FINTECH SDK - DUAL-LAYER LIVE SCANNER")
    print("===========================================================")

    # Initialize Video Capture or Generator via universal initializer
    if args.simulated:
        print("[INFO] Running in explicit SIMULATED mode for testing...")
        cap = SyntheticFrameGenerator()
        is_simulated = True
    else:
        cap, is_simulated = get_universal_camera_capture(args.cam)

    qr_detector = cv2.QRCodeDetector()
    fifo_queue = deque(maxlen=args.fifo_size)

    prev_time = time.time()
    fps = 0.0
    
    # Defaults
    l1_metrics = {'l1_score': 0.0, 'spatial_edge_density': 0.0, 'temporal_glare_var': 0.0, 'risk_level': 'NO QR'}
    l2_metrics = {
        'l2_score': 0.0,
        'crc_valid': True,
        'initiation_mode': '',
        'mcc': '',
        'merchant_name': '',
        'merchant_city': '',
        'parsed_tlv': {},
        'warnings': []
    }
    combined_score = 0.0
    combined_risk_level = 'NO QR'
    
    last_raw_qris_str = ""
    sim_count = 0

    print("[INFO] Press 'q' key in the video window to quit clean.\n")

    try:
        while True:
            ret, frame = cap.read()
            if not ret or frame is None:
                print("[INFO] Video stream ended or frame capture failed.")
                break

            curr_time = time.time()
            dt = curr_time - prev_time
            if dt > 0:
                fps = 0.9 * fps + 0.1 * (1.0 / dt)
            prev_time = curr_time

            # 1. Blur Gatekeeper Check
            gray = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            blur_var = cv2.Laplacian(gray, cv2.CV_64F).var()
            is_blurry = blur_var < args.blur_threshold

            # 2. QR Code Detection
            qr_bbox, poly_points, raw_qris_str = detect_qr_code(qr_detector, frame)

            if is_simulated:
                sim_count += 1
                # Mock QR box in center
                qr_bbox = (200, 140, 240, 240)
                poly_points = np.array([[200, 140], [440, 140], [440, 380], [200, 380]], dtype=np.int32)
                
                # Alternate QR payloads to cover various rules
                sim_mode = (sim_count // 30) % 4
                if sim_mode == 0:
                    # Case 1: Valid static QRIS
                    raw_qris_str = "00020101021126330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG MAKMUR6007JAKARTA63041B52"
                elif sim_mode == 1:
                    # Case 2: Tampered payload / CRC Fail
                    raw_qris_str = "00020101021126330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG HACKED6007JAKARTA63041B52"
                elif sim_mode == 2:
                    # Case 3: MCC misrepresentation (8661 Charity with "Toko" prefix)
                    raw_qris_str = "00020101021126330010A0000006020115ID10200000000015204866153033605802ID5919TOKO CHARITY BERKAH6007JAKARTA6304278F"
                else:
                    # Case 4: Dynamic QR (01=12) scanned in camera context
                    raw_qris_str = "00020101021226330010A0000006020115ID10200000000015204541153033605802ID5913WARUNG MAKMUR6007JAKARTA6304679C"

            # Technical Safeguard 3: Retain decoded QR string alongside score calculation
            if raw_qris_str:
                last_raw_qris_str = raw_qris_str

            # 3. Queue Management, Layer 1 & Layer 2 Evaluation
            if qr_bbox is not None and not is_blurry:
                fifo_queue.append(frame.copy())
                # Process Layer 1 edge anomaly and specular glare
                l1_metrics = process_layer1_edge(list(fifo_queue), qr_bbox)
                
                # Process Layer 2 payload rules
                l2_metrics = process_layer2_tlv(last_raw_qris_str, scan_context={"optical_type": "physical_camera_scan"})
                
                # Compute Combined Initial Ensemble Score
                l1_score = l1_metrics.get('l1_score', 0.0)
                l2_score = l2_metrics.get('l2_score', 0.0)
                crc_valid = l2_metrics.get('crc_valid', True)
                
                # Hard-veto applies if CRC invalid
                if not crc_valid:
                    combined_score = 1.0
                else:
                    combined_score = max(l1_score, l2_score)
                
                if combined_score < 0.35 and crc_valid:
                    combined_risk_level = 'LOW RISK'
                elif combined_score <= 0.70 and crc_valid:
                    combined_risk_level = 'CAUTION'
                else:
                    combined_risk_level = 'HIGH RISK'
                
                # Output/Log metrics
                print(f"[SCAN] Combined: {combined_score:.3f} | "
                      f"L1_Opt: {l1_score:.2f} | "
                      f"L2_EMV: {l2_score:.2f} | "
                      f"CRC: {crc_valid} | "
                      f"Merchant: '{l2_metrics.get('merchant_name', '')}' | "
                      f"Risk: {combined_risk_level}")
            elif is_blurry:
                # Clear queue when frame is blurry to maintain frame quality
                fifo_queue.clear()

            # 4. Render Real-Time UI HUD Overlay
            hud_frame = draw_hud(
                frame, fps, is_blurry, blur_var, qr_bbox, 
                l1_metrics, l2_metrics, combined_score, combined_risk_level, last_raw_qris_str
            )

            # Display frame window safely
            try:
                cv2.imshow("Anti Timpa QRIS Dual-Layer Scanner", hud_frame)
                key = cv2.waitKey(1) & 0xFF
                if key == ord('q'):
                    print("[INFO] 'q' key pressed. Exiting cleanly...")
                    break
            except Exception:
                # In headless environment without X display, limit and exit after some frames
                if is_simulated and sim_count >= 130:
                    print(f"[INFO] Headless test run completed {sim_count} frames successfully. Exiting.")
                    break

    except KeyboardInterrupt:
        print("[INFO] Interrupted by user.")
    finally:
        if not is_simulated and hasattr(cap, 'release'):
            cap.release()
        try:
            cv2.destroyAllWindows()
        except Exception:
            pass
        print("[INFO] Scanner closed cleanly.")


if __name__ == "__main__":
    args = parse_args()
    run_live_scanner(args)
