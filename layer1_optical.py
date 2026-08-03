"""
Anti Timpa QRIS Fintech SDK - Layer 1: Edge CNN & Optical Filter
Module: layer1_optical.py

Provides edge discontinuity analysis on the QR quiet zone margin and 
temporal specular glare variance analysis across a sliding FIFO frame sequence.
"""

import cv2
import numpy as np
from typing import List, Tuple, Dict, Any, Optional


def expand_bounding_box(
    qr_bbox: Tuple[int, int, int, int], 
    frame_width: int, 
    frame_height: int, 
    padding_percent: float = 0.10
) -> Tuple[int, int, int, int]:
    """
    Expands qr_bbox (x, y, w, h) by padding_percent per side (default 10% per side -> 20% total expansion),
    clamping coordinates strictly to [0, frame_width] and [0, frame_height].
    """
    x, y, w, h = qr_bbox
    pad_w = int(w * padding_percent)
    pad_h = int(h * padding_percent)

    x1_exp = max(0, x - pad_w)
    y1_exp = max(0, y - pad_h)
    x2_exp = min(frame_width, x + w + pad_w)
    y2_exp = min(frame_height, y + h + pad_h)

    w_exp = max(1, x2_exp - x1_exp)
    h_exp = max(1, y2_exp - y1_exp)

    return (x1_exp, y1_exp, w_exp, h_exp)


def create_quiet_zone_margin_mask(
    expanded_shape: Tuple[int, int],
    inner_rel_bbox: Tuple[int, int, int, int]
) -> np.ndarray:
    """
    Creates a binary mask (1 for Quiet Zone margin, 0 for inner QR content)
    where outer padded margin is analyzed for sticker boundary discontinuities.
    """
    h_exp, w_exp = expanded_shape
    inner_x, inner_y, inner_w, inner_h = inner_rel_bbox

    mask = np.ones((h_exp, w_exp), dtype=np.uint8)

    # Clamp inner bbox coordinates relative to crop
    ix1 = max(0, min(w_exp, inner_x))
    iy1 = max(0, min(h_exp, inner_y))
    ix2 = max(0, min(w_exp, inner_x + inner_w))
    iy2 = max(0, min(h_exp, inner_y + inner_h))

    if ix2 > ix1 and iy2 > iy1:
        mask[iy1:iy2, ix1:ix2] = 0

    # If quiet zone margin is empty (0 area), default to full ROI mask
    if np.sum(mask) == 0:
        mask = np.ones((h_exp, w_exp), dtype=np.uint8)

    return mask


def compute_spatial_edge_density(
    crop_gray: np.ndarray,
    margin_mask: np.ndarray,
    sobel_threshold: float = 100.0
) -> float:
    """
    Computes high-frequency spatial edge gradients using Sobel operators:
    G = sqrt(G_x^2 + G_y^2).
    Binarizes gradient magnitude (G > sobel_threshold) before computing Edge Density
    focused heavily on the Quiet Zone margin to resist ambient lighting variations.
    """
    if crop_gray.size == 0 or np.sum(margin_mask) == 0:
        return 0.0

    # Compute Sobel Gradients
    sobel_x = cv2.Sobel(crop_gray, cv2.CV_64F, 1, 0, ksize=3)
    sobel_y = cv2.Sobel(crop_gray, cv2.CV_64F, 0, 1, ksize=3)
    grad_mag = np.sqrt(sobel_x**2 + sobel_y**2)

    # Safeguard 1: Binarize gradient magnitude above sobel_threshold
    binary_edges = (grad_mag > sobel_threshold).astype(np.uint8)

    # Safeguard 2: Focus spatial edge detection heavily on the Quiet Zone margin
    margin_edges = binary_edges * margin_mask
    margin_pixel_count = float(np.sum(margin_mask))

    edge_density = float(np.sum(margin_edges)) / (margin_pixel_count + 1e-6)
    return edge_density


def compute_temporal_glare_variance(
    frame_sequence: List[np.ndarray],
    qr_bbox: Tuple[int, int, int, int],
    glare_threshold: int = 220
) -> float:
    """
    Measures specular highlight (glare) intensity variance (> glare_threshold)
    across the sliding FIFO frame sequence queue.
    """
    if not frame_sequence:
        return 0.0

    glare_ratios = []
    for frame in frame_sequence:
        fh, fw = frame.shape[:2]
        x_exp, y_exp, w_exp, h_exp = expand_bounding_box(qr_bbox, fw, fh)
        crop = frame[y_exp:y_exp + h_exp, x_exp:x_exp + w_exp]

        if crop.size == 0:
            continue

        if len(crop.shape) == 3:
            gray = cv2.cvtColor(crop, cv2.COLOR_BGR2GRAY)
        else:
            gray = crop

        # Proportion of bright specular highlight pixels
        glare_pixels = np.sum(gray > glare_threshold)
        glare_ratio = float(glare_pixels) / float(gray.size + 1e-6)
        glare_ratios.append(glare_ratio)

    if len(glare_ratios) <= 1:
        return 0.0

    glare_var = float(np.var(glare_ratios))
    return glare_var


def process_layer1_edge(
    frame_sequence: List[np.ndarray],
    qr_bbox: Tuple[int, int, int, int]
) -> Dict[str, Any]:
    """
    Main Layer 1 analysis entry point.
    Processes frame_sequence (FIFO queue of clear frames) and qr_bbox tuple (x, y, w, h).

    Returns a dictionary:
    {
        'l1_score': float [0.0 - 1.0],
        'spatial_edge_density': float,
        'temporal_glare_var': float,
        'risk_level': str ('LOW RISK' | 'CAUTION' | 'HIGH RISK'),
        'expanded_bbox': (x_exp, y_exp, w_exp, h_exp)
    }
    """
    if not frame_sequence or qr_bbox is None or qr_bbox[2] <= 0 or qr_bbox[3] <= 0:
        return {
            'l1_score': 0.0,
            'spatial_edge_density': 0.0,
            'temporal_glare_var': 0.0,
            'risk_level': 'LOW RISK',
            'expanded_bbox': (0, 0, 0, 0)
        }

    latest_frame = frame_sequence[-1]
    fh, fw = latest_frame.shape[:2]

    # 1. 20% expanded bounding box (10% per side padding)
    x_exp, y_exp, w_exp, h_exp = expand_bounding_box(qr_bbox, fw, fh, padding_percent=0.10)
    crop_bgr = latest_frame[y_exp:y_exp + h_exp, x_exp:x_exp + w_exp]

    if crop_bgr.size == 0:
        return {
            'l1_score': 0.0,
            'spatial_edge_density': 0.0,
            'temporal_glare_var': 0.0,
            'risk_level': 'LOW RISK',
            'expanded_bbox': (x_exp, y_exp, w_exp, h_exp)
        }

    crop_gray = cv2.cvtColor(crop_bgr, cv2.COLOR_BGR2GRAY) if len(crop_bgr.shape) == 3 else crop_bgr

    # Relative coordinates of original QR inside expanded crop
    x, y, w, h = qr_bbox
    inner_rel_bbox = (x - x_exp, y - y_exp, w, h)

    # 2. Quiet Zone Margin Mask & Spatial Edge Density (Safeguards 1 & 2)
    margin_mask = create_quiet_zone_margin_mask((h_exp, w_exp), inner_rel_bbox)
    edge_density = compute_spatial_edge_density(crop_gray, margin_mask, sobel_threshold=100.0)

    # 3. Temporal Specular Glare Variance across FIFO frame sequence
    glare_var = compute_temporal_glare_variance(frame_sequence, qr_bbox, glare_threshold=220)

    # 4. Score Normalization & Risk Ensemble calculation
    # Clean paper quiet zone margin edge density is typically < 0.04.
    # Sticker overlays introduce physical edges giving edge density >= 0.12.
    s_edge = float(np.clip(edge_density / 0.15, 0.0, 1.0))

    # Glare variance across motion for paper is ~0.0, while glossy plastic sticker is > 0.002
    s_glare = float(np.clip(glare_var / 0.003, 0.0, 1.0))

    # Ensemble score (65% spatial edge anomaly in quiet zone margin, 35% temporal glare variance)
    l1_score = float(np.clip(0.65 * s_edge + 0.35 * s_glare, 0.0, 1.0))

    if l1_score < 0.40:
        risk_level = 'LOW RISK'
    elif l1_score <= 0.70:
        risk_level = 'CAUTION'
    else:
        risk_level = 'HIGH RISK'

    return {
        'l1_score': l1_score,
        'spatial_edge_density': edge_density,
        'temporal_glare_var': glare_var,
        'risk_level': risk_level,
        'expanded_bbox': (x_exp, y_exp, w_exp, h_exp)
    }
