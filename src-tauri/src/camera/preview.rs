//! Camera preview encoding.
//!
//! Getting a live preview into the webview has one hard constraint: the
//! transport is IPC, and raw frames are far too large for it.
//!
//! A 640x480 RGB frame is ~900 KB of bytes. Serialized to JSON as a numeric
//! array it becomes roughly 3-4 MB of text, per frame. At even 10 fps that is
//! tens of megabytes per second through `invoke`, which would pin a CPU core
//! and stutter the UI.
//!
//! So preview frames are:
//!
//! 1. Downscaled to a small preview width (default 480 px).
//! 2. JPEG-encoded at moderate quality (~30-60 KB).
//! 3. Base64-encoded into a `data:` URL the DOM can assign to `<img src>`.
//!
//! That lands at roughly 40-80 KB per frame, a ~50x reduction. Combined with
//! a frame-rate cap in the UI, preview becomes cheap enough to run alongside
//! the one-shot analysis capture.
//!
//! The preview is never used for analysis. Analysis always runs on the
//! full-resolution frame in Rust. Sending a downscaled JPEG to the UI and
//! analysing it there would break the Layer 1 optical checks, which depend on
//! fine edge detail in the quiet zone.

use crate::camera::Frame;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::RgbImage;

/// Default preview width. Chosen so the preview is crisp on a phone-sized
/// window without inflating each frame past ~60 KB.
pub const DEFAULT_PREVIEW_WIDTH: u32 = 480;

/// JPEG quality for preview. 70 is the point where QR finder patterns stay
/// visually recognisable for framing while staying small.
const PREVIEW_JPEG_QUALITY: u8 = 70;

/// Encodes a frame as a `data:image/jpeg;base64,...` URL.
///
/// `max_width` bounds the output width; height follows the aspect ratio. A
/// frame already narrower than `max_width` is not upscaled, since upscaling
/// would cost bytes without adding information.
pub fn to_data_url(frame: &Frame, max_width: u32) -> Result<String, String> {
    if frame.width == 0 || frame.height == 0 {
        return Err("frame kosong".to_string());
    }

    let src = RgbImage::from_raw(frame.width, frame.height, frame.rgb.clone())
        .ok_or_else(|| "gagal menyusun buffer RGB untuk preview".to_string())?;

    let target_width = if max_width == 0 || frame.width <= max_width {
        frame.width
    } else {
        max_width
    };

    // Preserve aspect ratio, rounding to at least 1px so a very wide and short
    // frame cannot collapse to zero height (which would fail encoding).
    let target_height = if frame.width == target_width {
        frame.height
    } else {
        let scaled = (frame.height as f64 * target_width as f64 / frame.width as f64).round();
        (scaled as u32).max(1)
    };

    let preview = if (target_width, target_height) == (frame.width, frame.height) {
        src
    } else {
        // Triangle filter is the right trade-off here: cheap enough for a live
        // preview, and smoother than nearest-neighbour which would make QR
        // finder patterns alias badly at small sizes.
        image::imageops::resize(&src, target_width, target_height, FilterType::Triangle)
    };

    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, PREVIEW_JPEG_QUALITY)
        .encode(preview.as_raw(), target_width, target_height, image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("encode JPEG preview gagal: {e}"))?;

    Ok(format!("data:image/jpeg;base64,{}", BASE64.encode(&jpeg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_frame(w: u32, h: u32, color: [u8; 3]) -> Frame {
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..(w * h) {
            rgb.extend_from_slice(&color);
        }
        Frame::new(w, h, rgb).unwrap()
    }

    /// Extracts the raw bytes back out of a data URL, so tests can assert on
    /// the actual encoded payload rather than just the prefix.
    fn decode_payload(data_url: &str) -> Vec<u8> {
        let b64 = data_url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("must be a JPEG data URL");
        BASE64.decode(b64).expect("payload must be valid base64")
    }

    #[test]
    fn produces_jpeg_data_url() {
        let frame = solid_frame(100, 50, [200, 100, 50]);
        let url = to_data_url(&frame, 480).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"), "got: {url}");
    }

    #[test]
    fn payload_decodes_to_jpeg_magic_bytes() {
        // JPEG files start with SOI (0xFFD8) and end with EOI (0xFFD9).
        // Asserting on these catches an encoder that silently emits another
        // format, which the browser would refuse to render.
        let frame = solid_frame(64, 64, [10, 20, 30]);
        let bytes = decode_payload(&to_data_url(&frame, 480).unwrap());

        assert_eq!(&bytes[0..2], &[0xFF, 0xD8], "must start with JPEG SOI");
        assert_eq!(
            &bytes[bytes.len() - 2..],
            &[0xFF, 0xD9],
            "must end with JPEG EOI"
        );
    }

    #[test]
    fn large_frame_is_downscaled_and_small() {
        // A full-resolution frame must shrink dramatically. Without
        // downscaling, IPC transport is not viable.
        let frame = solid_frame(1280, 720, [80, 90, 100]);
        let url = to_data_url(&frame, DEFAULT_PREVIEW_WIDTH).unwrap();
        let bytes = decode_payload(&url);

        assert!(
            bytes.len() < 60_000,
            "preview should stay well under 60 KB, got {} bytes",
            bytes.len()
        );

        let decoded = image::load_from_memory(&bytes).expect("must decode as an image");
        assert_eq!(
            decoded.width(),
            DEFAULT_PREVIEW_WIDTH,
            "width must be capped to the preview width"
        );
    }

    #[test]
    fn small_frame_is_not_upscaled() {
        // Upscaling would inflate the payload for zero visual benefit.
        let frame = solid_frame(120, 90, [5, 5, 5]);
        let bytes = decode_payload(&to_data_url(&frame, DEFAULT_PREVIEW_WIDTH).unwrap());
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (120, 90));
    }

    #[test]
    fn aspect_ratio_is_preserved() {
        let frame = solid_frame(1600, 900, [1, 2, 3]);
        let bytes = decode_payload(&to_data_url(&frame, 400).unwrap());
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (400, 225));
    }

    #[test]
    fn degenerate_aspect_ratio_does_not_collapse_to_zero() {
        // A very wide, very short frame would round to height 0 and fail
        // encoding. Clamping to 1px keeps it valid.
        let frame = solid_frame(2000, 3, [9, 9, 9]);
        let url = to_data_url(&frame, 100);
        assert!(url.is_ok(), "must not fail on extreme aspect ratio: {url:?}");

        let bytes = decode_payload(&url.unwrap());
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.height() >= 1, "height must never be zero");
    }

    #[test]
    fn empty_frame_is_rejected() {
        let frame = Frame {
            width: 0,
            height: 0,
            rgb: Vec::new(),
        };
        assert!(to_data_url(&frame, 480).is_err());
    }
}
