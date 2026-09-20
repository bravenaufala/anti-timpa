//! QR decoding.
//!
//! Replaces the `cv2.QRCodeDetector` + `pyzbar` combination from
//! `scanner_core.py::detect_qr`. The old code needed two decoders and a
//! multi-scale/CLAHE fallback chain because OpenCV alone frequently found a
//! QR's location but failed to decode its payload — especially for small QRs
//! inside large gallery photos.
//!
//! `rqrr` is a pure-Rust detector that supports multi-scale preparation
//! directly, which collapses that whole fallback chain into one call.

use crate::camera::Frame;
use image::GrayImage;

/// A decoded QR: payload plus its bounding box in the source frame.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QrHit {
    pub payload: String,
    /// Bounding box in pixel coordinates: x, y, width, height.
    pub bbox: [u32; 4],
}

/// Attempts to decode a QR from a frame.
///
/// Returns `Ok(None)` when the image is valid but contains no readable QR —
/// that is a normal outcome, not an error, and the UI should say
/// "arahkan lebih dekat" rather than surface a failure.
pub fn decode(frame: &Frame) -> Result<Option<QrHit>, String> {
    if frame.width == 0 || frame.height == 0 {
        return Err("frame kosong".into());
    }

    let gray = GrayImage::from_raw(frame.width, frame.height, frame.to_gray())
        .ok_or_else(|| "gagal menyusun buffer grayscale".to_string())?;

    let mut img = rqrr::PreparedImage::prepare(gray);
    let grids = img.detect_grids();

    for grid in grids {
        // Only report a hit when the payload actually decodes. Returning a
        // location with an empty payload would make a correctly-printed but
        // merely unreadable QR look like a tampered one — the exact bug the
        // old code guarded against with its `raw_qris_str` check.
        match grid.decode() {
            Ok((_meta, content)) if !content.is_empty() => {
                // `bounds` is the four corner points of the QR in source-image
                // coordinates. Reduce them to an axis-aligned bounding box,
                // which is what Layer 1's quiet-zone analysis expects.
                let bbox = bounds_to_bbox(&grid.bounds);
                return Ok(Some(QrHit { payload: content, bbox }));
            }
            _ => continue,
        }
    }

    Ok(None)
}

/// Reduces the QR's four corner points to an axis-aligned bounding box,
/// clamped to the frame so downstream slicing can never go out of range.
fn bounds_to_bbox(bounds: &[rqrr::Point; 4]) -> [u32; 4] {
    let xs = bounds.iter().map(|p| p.x);
    let ys = bounds.iter().map(|p| p.y);

    let min_x = xs.clone().min().unwrap_or(0);
    let max_x = xs.max().unwrap_or(0);
    let min_y = ys.clone().min().unwrap_or(0);
    let max_y = ys.max().unwrap_or(0);

    // rqrr reports point coordinates as usize in image space; the cast to i64
    // keeps the width/height arithmetic from underflowing on degenerate input.
    let min_x = min_x as i64;
    let min_y = min_y as i64;
    let max_x = max_x as i64;
    let max_y = max_y as i64;

    [
        min_x.max(0) as u32,
        min_y.max(0) as u32,
        (max_x - min_x).max(0) as u32,
        (max_y - min_y).max(0) as u32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A QR rendered by hand would be tedious; instead assert the contract
    /// that matters: a blank frame yields "no QR", not an error.
    #[test]
    fn blank_frame_yields_no_hit() {
        let frame = Frame::new(64, 64, vec![255u8; 64 * 64 * 3]).unwrap();
        assert!(decode(&frame).unwrap().is_none());
    }

    #[test]
    fn empty_frame_is_rejected() {
        let frame = Frame {
            width: 0,
            height: 0,
            rgb: Vec::new(),
        };
        assert!(decode(&frame).is_err());
    }

    #[test]
    fn mismatched_buffer_length_is_rejected_at_construction() {
        // Guards the invariant the whole pipeline relies on: a Frame always
        // holds exactly width * height * 3 bytes.
        let err = Frame::new(10, 10, vec![0u8; 10]).unwrap_err();
        assert!(err.to_string().contains("tidak valid"), "got: {err}");
    }

    #[test]
    fn bbox_spans_all_four_corners() {
        let bounds = [
            rqrr::Point { x: 10, y: 20 },
            rqrr::Point { x: 50, y: 20 },
            rqrr::Point { x: 50, y: 80 },
            rqrr::Point { x: 10, y: 80 },
        ];
        assert_eq!(bounds_to_bbox(&bounds), [10, 20, 40, 60]);
    }

    #[test]
    fn bbox_handles_unsorted_corners() {
        // rqrr does not guarantee corner ordering, so the reducer must not
        // assume the first point is the top-left.
        let bounds = [
            rqrr::Point { x: 50, y: 80 },
            rqrr::Point { x: 10, y: 20 },
            rqrr::Point { x: 50, y: 20 },
            rqrr::Point { x: 10, y: 80 },
        ];
        assert_eq!(bounds_to_bbox(&bounds), [10, 20, 40, 60]);
    }
}
