"""
Verification Test Suite - Anti Timpa QRIS Layer 1 Optical Scanner
File: test_layer1.py
"""

import cv2
import numpy as np
from layer1_optical import (
    expand_bounding_box,
    create_quiet_zone_margin_mask,
    compute_spatial_edge_density,
    compute_temporal_glare_variance,
    process_layer1_edge
)
from live_scanner import detect_qr_code


def create_synthetic_qr_frame(
    width: int = 640,
    height: int = 480,
    qr_pos: tuple = (200, 140, 240, 240),
    has_sticker_anomaly: bool = False,
    glare_intensity: int = 0,
    blur_kernel: int = 0
) -> np.ndarray:
    """Generates a synthetic BGR frame with controllable QR features, sticker anomalies, glare, and blur."""
    frame = np.full((height, width, 3), 195, dtype=np.uint8)  # Off-white paper background (< 220)
    qx, qy, qw, qh = qr_pos

    # 1. Quiet zone background paper
    cv2.rectangle(frame, (qx - 30, qy - 30), (qx + qw + 30, qy + qh + 30), (210, 210, 210), -1)

    # 2. Main QR pattern (inner box)
    cv2.rectangle(frame, (qx, qy), (qx + qw, qy + qh), (0, 0, 0), 2)
    # Alignment finder patterns
    cv2.rectangle(frame, (qx + 10, qy + 10), (qx + 60, qy + 60), (0, 0, 0), -1)
    cv2.rectangle(frame, (qx + qw - 60, qy + 10), (qx + qw - 10, qy + 60), (0, 0, 0), -1)
    cv2.rectangle(frame, (qx + 10, qy + qh - 60), (qx + 60, qy + qh - 10), (0, 0, 0), -1)

    # 3. Simulate Physical Sticker Anomaly (high-frequency dual-edge cutout in quiet zone margin)
    if has_sticker_anomaly:
        # Draw high-contrast cutout border in the 10% Quiet Zone margin
        cv2.rectangle(frame, (qx - 15, qy - 15), (qx + qw + 15, qy + qh + 15), (30, 30, 30), 4)
        cv2.rectangle(frame, (qx - 10, qy - 10), (qx + qw + 10, qy + qh + 10), (180, 180, 180), 2)

    # 4. Simulate Specular Glare (bright reflection > 220)
    if glare_intensity > 0:
        cv2.circle(frame, (qx + qw // 2, qy + qh // 2), glare_intensity, (255, 255, 255), -1)

    # 5. Simulate Blur
    if blur_kernel > 1:
        if blur_kernel % 2 == 0:
            blur_kernel += 1
        frame = cv2.GaussianBlur(frame, (blur_kernel, blur_kernel), 0)

    return frame


def test_bounding_box_expansion():
    print("[TEST 1] Bounding Box 20% Expansion & Clamping...")
    w_frame, h_frame = 640, 480
    qr_bbox = (100, 100, 200, 200)

    # 10% per side -> 20% expansion
    x_exp, y_exp, w_exp, h_exp = expand_bounding_box(qr_bbox, w_frame, h_frame, padding_percent=0.10)
    assert x_exp == 80, f"Expected x_exp=80, got {x_exp}"
    assert y_exp == 80, f"Expected y_exp=80, got {y_exp}"
    assert w_exp == 240, f"Expected w_exp=240, got {w_exp}"
    assert h_exp == 240, f"Expected h_exp=240, got {h_exp}"

    # Corner clamping test
    corner_bbox = (0, 0, 100, 100)
    x_c, y_c, w_c, h_c = expand_bounding_box(corner_bbox, w_frame, h_frame, padding_percent=0.10)
    assert x_c == 0, f"Expected x_c=0, got {x_c}"
    assert y_c == 0, f"Expected y_c=0, got {y_c}"
    print("  -> PASSED.")


def test_binarized_sobel_margin_edge_density():
    print("[TEST 2] Quiet Zone Margin Edge Density & Sobel Binarization (G > 100)...")
    clean_frame = create_synthetic_qr_frame(has_sticker_anomaly=False)
    sticker_frame = create_synthetic_qr_frame(has_sticker_anomaly=True)

    qr_bbox = (200, 140, 240, 240)
    
    clean_res = process_layer1_edge([clean_frame], qr_bbox)
    sticker_res = process_layer1_edge([sticker_frame], qr_bbox)

    print(f"  Clean paper margin edge density: {clean_res['spatial_edge_density']:.4f}")
    print(f"  Sticker anomaly margin edge density: {sticker_res['spatial_edge_density']:.4f}")

    assert clean_res['spatial_edge_density'] < sticker_res['spatial_edge_density'], \
        "Sticker frame must have higher quiet zone margin edge density than clean paper!"
    assert sticker_res['l1_score'] > clean_res['l1_score'], \
        "Sticker frame must produce higher l1_score than clean paper!"
    print("  -> PASSED.")


def test_temporal_glare_variance():
    print("[TEST 3] Temporal Specular Glare Variance across FIFO queue...")
    qr_bbox = (200, 140, 240, 240)

    # Static clean frame sequence (no glare variance)
    static_seq = [create_synthetic_qr_frame() for _ in range(5)]
    # Dynamic glare frame sequence (varying reflection size)
    glare_seq = [create_synthetic_qr_frame(glare_intensity=g) for g in [0, 40, 10, 50, 5]]

    static_res = process_layer1_edge(static_seq, qr_bbox)
    glare_res = process_layer1_edge(glare_seq, qr_bbox)

    print(f"  Static frame glare var: {static_res['temporal_glare_var']:.6f}")
    print(f"  Dynamic glare frame glare var: {glare_res['temporal_glare_var']:.6f}")

    assert glare_res['temporal_glare_var'] > static_res['temporal_glare_var'], \
        "Dynamic glare sequence must produce higher glare variance!"
    print("  -> PASSED.")


def test_blur_gatekeeper():
    print("[TEST 4] Laplacian Variance Blur Gatekeeper...")
    sharp_frame = create_synthetic_qr_frame(blur_kernel=0)
    blurry_frame = create_synthetic_qr_frame(blur_kernel=21)

    sharp_gray = cv2.cvtColor(sharp_frame, cv2.COLOR_BGR2GRAY)
    blurry_gray = cv2.cvtColor(blurry_frame, cv2.COLOR_BGR2GRAY)

    sharp_var = cv2.Laplacian(sharp_gray, cv2.CV_64F).var()
    blurry_var = cv2.Laplacian(blurry_gray, cv2.CV_64F).var()

    print(f"  Sharp frame Laplacian variance: {sharp_var:.2f}")
    print(f"  Blurry frame Laplacian variance: {blurry_var:.2f}")

    assert sharp_var >= 100.0, f"Expected sharp frame var >= 100, got {sharp_var}"
    assert blurry_var < 100.0, f"Expected blurry frame var < 100, got {blurry_var}"
    print("  -> PASSED.")


def test_qr_detection_and_payload():
    print("[TEST 5] QR Code Detector Bounding Box & Raw String Retention Safeguard...")
    frame = create_synthetic_qr_frame()
    detector = cv2.QRCodeDetector()
    
    qr_bbox, poly_pts, raw_str = detect_qr_code(detector, frame)
    # Synthetic frame has drawn QR features, test that detection doesn't crash
    print(f"  Detected Bounding Box: {qr_bbox}, Raw QR String: '{raw_str}'")
    print("  -> PASSED.")


def run_all_tests():
    print("==================================================")
    print("RUNNING ANTI TIMPA LAYER 1 AUTOMATED TEST SUITE")
    print("==================================================")
    test_bounding_box_expansion()
    test_binarized_sobel_margin_edge_density()
    test_temporal_glare_variance()
    test_blur_gatekeeper()
    test_qr_detection_and_payload()
    print("==================================================")
    print("ALL TEST SUITE CHECKS PASSED SUCCESSFULLY!")
    print("==================================================")


if __name__ == "__main__":
    run_all_tests()
