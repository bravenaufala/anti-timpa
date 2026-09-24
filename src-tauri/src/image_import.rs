//! Image import — running the optical pipeline on a photo instead of a camera.
//!
//! # Why this exists, stated honestly
//!
//! This is a **validation and demonstration path, not a product feature.** The
//! overlay attack happens at a physical QR in front of a camera, and the
//! camera path in `lib.rs::capture_and_analyze` is the one that addresses it.
//!
//! What this module is actually for:
//!
//! 1. **Fitting the Layer 1 thresholds against real photographs.** The camera
//!    path can only be exercised on a device with a physical QRIS sticker.
//!    Importing a photo makes it possible to check — and falsify — the
//!    constants that were fitted on synthetic fixtures.
//! 2. **Demonstrating Layer 1 without hardware.** A laptop demo with no camera
//!    and no printed sticker can still show the optical layer working.
//!
//! It is kept in a separate module, with its own honest doc comment, so nobody
//! mistakes it for the primary detection path.
//!
//! # What an imported photo cannot do
//!
//! The temporal glare signal needs a *burst* of frames of the same untouched
//! scene. A single imported photo has no burst, so
//! [`imported_frame_has_burst`] is always `false` and Layer 1 runs with its
//! spatial signals only. Pretending otherwise — for instance by re-analysing the
//! same image four times and reporting the resulting zero variance as
//! "no glare risk" — would manufacture a measurement. Instead the caller is
//! told the temporal check was skipped.

use crate::camera::Frame;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader, RgbImage};

/// Upper bound on the longest side after import.
///
/// A modern phone photo is 4000+ px on the long edge and roughly 48 MB decoded.
/// Layer 1's metrics are scale-invariant because the detection ring is expressed
/// as a fraction of the symbol, so downscaling costs no signal — but it keeps
/// the imported frame in the same resolution regime as a camera frame, which is
/// what makes measurements from the two paths comparable at all.
pub const MAX_IMPORT_DIMENSION: u32 = 1600;

/// Lower bound. Below this a QR is unlikely to survive decoding, and the
/// quiet-zone ring would be only a few pixels wide.
pub const MIN_IMPORT_DIMENSION: u32 = 32;

/// Metadata about an imported image, surfaced so the UI can show what was
/// actually analysed rather than just the filename.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportInfo {
    pub original_width: u32,
    pub original_height: u32,
    pub width: u32,
    pub height: u32,
    /// True when the frame was downscaled to stay under [`MAX_IMPORT_DIMENSION`].
    pub downscaled: bool,
    /// Always `false` for a single imported image — see the module docs.
    pub imported_frame_has_burst: bool,
}

/// A decoded image turned into the RGB [`Frame`] the analysis layers consume.
#[derive(Debug)]
pub struct ImportedImage {
    pub frame: Frame,
    pub info: ImportInfo,
}

/// Decodes an image from bytes and scales it into the analysis resolution band.
///
/// Decoding is format-agnostic (`image` sniffs the container), so the caller can
/// hand over whatever the user picked.
///
/// # Errors
///
/// Returns a message suitable for display when the bytes are not a decodable
/// image, or when the image is too small for the optical analysis to be
/// meaningful.
pub fn load_from_bytes(bytes: &[u8]) -> Result<ImportedImage, String> {
    if bytes.is_empty() {
        return Err("berkas kosong".to_string());
    }

    // `with_guessed_format` is required rather than optional: a file picked from
    // disk often has no useful extension, and without format sniffing the reader
    // would either fail or, worse, mis-decode.
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("gagal membaca format gambar: {e}"))?;

    let decoded: DynamicImage = reader
        .decode()
        .map_err(|e| format!("gagal mendekode gambar (format tidak didukung atau berkas rusak): {e}"))?;

    let (original_width, original_height) = (decoded.width(), decoded.height());
    if original_width < MIN_IMPORT_DIMENSION || original_height < MIN_IMPORT_DIMENSION {
        return Err(format!(
            "gambar terlalu kecil ({original_width}x{original_height}px); \
             minimal {MIN_IMPORT_DIMENSION}x{MIN_IMPORT_DIMENSION}px agar analisis optik bermakna"
        ));
    }

    let longest = original_width.max(original_height);
    let (target_w, target_h, downscaled) = if longest > MAX_IMPORT_DIMENSION {
        let scale = MAX_IMPORT_DIMENSION as f64 / longest as f64;
        (
            ((original_width as f64 * scale).round() as u32).max(1),
            ((original_height as f64 * scale).round() as u32).max(1),
            true,
        )
    } else {
        (original_width, original_height, false)
    };

    // Resize only when needed: an unnecessary resample would blur the fine quiet
    // zone detail that Layer 1 exists to measure.
    let rgb: RgbImage = if downscaled {
        // Triangle is the right trade-off: it avoids the aliasing that
        // nearest-neighbour would introduce on the QR's module grid, which would
        // itself look like an edge signal.
        DynamicImage::ImageRgb8(decoded.to_rgb8())
            .resize_exact(target_w, target_h, FilterType::Triangle)
            .to_rgb8()
    } else {
        decoded.to_rgb8()
    };

    let (width, height) = (rgb.width(), rgb.height());
    let frame = Frame::new(width, height, rgb.into_raw())
        .map_err(|e| format!("gagal menyusun frame dari gambar: {e}"))?;

    Ok(ImportedImage {
        frame,
        info: ImportInfo {
            original_width,
            original_height,
            width,
            height,
            downscaled,
            imported_frame_has_burst: false,
        },
    })
}

/// Renders a frame back to PNG bytes.
///
/// Used by the round-trip tests to prove that an encoded, decoded, and rescaled
/// image still decodes as a QR, and by the `probe_import` diagnostic. Without
/// such a test the import path could silently break and the failure would look
/// like "this photo contains no QR".
#[cfg(any(test, feature = "qr-encode"))]
pub fn encode_png(frame: &Frame) -> Result<Vec<u8>, String> {
    let img = RgbImage::from_raw(frame.width, frame.height, frame.rgb.clone())
        .ok_or_else(|| "gagal menyusun buffer RGB".to_string())?;
    let mut out = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("encode PNG gagal: {e}"))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    // `qrcode`-backed tests need the encoder, which is off by default so the
    // shipping binary does not link it. Gating the whole module keeps a plain
    // `cargo test` from failing to compile.
    #![cfg(feature = "qr-encode")]

    use super::*;
    use crate::camera::synthetic::{SyntheticBackend, SyntheticSpec};
    use crate::camera::CameraBackend;

    fn synthetic_frame(spec: SyntheticSpec) -> Frame {
        SyntheticBackend::with_spec(spec).capture().unwrap()
    }

    fn default_spec() -> SyntheticSpec {
        SyntheticSpec {
            glare_radius: 1,
            qr_payload: Some(
                "00020101021126330010A0000006020115ID10200000000015204541153033605802ID\
                 5913WARUNG MAKMUR6007JAKARTA63041B52"
                    .to_string(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn round_trip_preserves_dimensions_when_under_the_cap() {
        let frame = synthetic_frame(default_spec());
        let png = encode_png(&frame).unwrap();
        let imported = load_from_bytes(&png).unwrap();

        assert_eq!(imported.info.original_width, 640);
        assert_eq!(imported.info.original_height, 480);
        assert_eq!((imported.frame.width, imported.frame.height), (640, 480));
        assert!(!imported.info.downscaled);
        assert_eq!(imported.frame.rgb.len(), 640 * 480 * 3);
    }

    #[test]
    fn large_image_is_downscaled_into_the_analysis_band() {
        // 3200 px is beyond the cap, so it must come back at MAX_IMPORT_DIMENSION
        // on the long edge, with the aspect ratio intact.
        let big = Frame::new(3200, 1600, vec![128u8; 3200 * 1600 * 3]).unwrap();
        let png = encode_png(&big).unwrap();
        let imported = load_from_bytes(&png).unwrap();

        assert!(imported.info.downscaled);
        assert_eq!(imported.info.original_width, 3200);
        assert_eq!(imported.frame.width, MAX_IMPORT_DIMENSION);
        assert_eq!(imported.frame.height, MAX_IMPORT_DIMENSION / 2);
    }

    #[test]
    fn a_large_qr_survives_downscaling_and_the_round_trip() {
        // The realistic import case: a phone photo far above the cap must still
        // decode after being scaled down. This is the property that makes the
        // import path usable for validating the detector against real photos.
        let mut spec = default_spec();
        spec.width = 3200;
        spec.height = 2400;
        spec.qr_bbox = (1000, 700, 1200, 1200);
        let frame = synthetic_frame(spec);

        let png = encode_png(&frame).unwrap();
        let imported = load_from_bytes(&png).unwrap();
        assert!(imported.info.downscaled, "3200 px must be downscaled");

        let hit = crate::qr::decode(&imported.frame)
            .expect("decode should not error")
            .expect("the QR must survive downscaling from 3200 px");
        assert!(hit.payload.starts_with("00020101"), "got: {}", hit.payload);
    }

    #[test]
    fn a_qr_survives_encode_decode_rescale_round_trip() {
        // The property that makes this path useful to validate against: an
        // imported photo still yields a decodable QR. If this breaks, the import
        // path would look like "no QR in the photo" instead of a bug.
        let frame = synthetic_frame(default_spec());
        let png = encode_png(&frame).unwrap();
        let imported = load_from_bytes(&png).unwrap();

        let hit = crate::qr::decode(&imported.frame)
            .expect("decode should not error")
            .expect("a synthetic QR should survive the round trip");
        assert!(!hit.payload.is_empty());
    }

    #[test]
    fn a_sticker_frame_survives_the_round_trip_and_still_scores_anomalous() {
        // The end-to-end claim of the import path: importing the *sticker* case
        // must still decode AND still produce a Layer 1 finding. If either half
        // fails, the import path cannot be used to validate the detector.
        let frame = synthetic_frame(SyntheticSpec {
            sticker_anomaly: true,
            ..default_spec()
        });
        let png = encode_png(&frame).unwrap();
        let imported = load_from_bytes(&png).unwrap();

        let hit = crate::qr::decode(&imported.frame)
            .unwrap()
            .expect("sticker frame should still decode; the sticker covers only the margin");
        let (x, y, w, h) = (hit.bbox[0], hit.bbox[1], hit.bbox[2], hit.bbox[3]);
        let l1 = crate::layer1_optical::analyze_burst(std::slice::from_ref(&imported.frame), (x, y, w, h));

        assert!(l1.ran(), "layer 1 must run on an imported frame");
        assert!(
            l1.spatial_edge_density > 0.0,
            "an imported sticker frame must show margin edges, got {}",
            l1.spatial_edge_density
        );
    }

    /// The finding that motivated giving the synthetic backend a real symbol.
    ///
    /// The sticker is drawn in the margin, *outside* the symbol's module area, so
    /// it must not disturb decoding. That is what makes the attack interesting:
    /// the payload still reads cleanly while the object has been tampered with.
    #[test]
    fn the_sticker_does_not_prevent_decoding() {
        let clean = synthetic_frame(default_spec());
        let sticker = synthetic_frame(SyntheticSpec {
            sticker_anomaly: true,
            ..default_spec()
        });

        let clean_hit = crate::qr::decode(&clean).unwrap().expect("clean must decode");
        let sticker_hit = crate::qr::decode(&sticker)
            .unwrap()
            .expect("sticker must not prevent decoding — the payload is unchanged");

        assert_eq!(
            clean_hit.payload, sticker_hit.payload,
            "an overlay does not alter the payload, which is exactly why \
             payload-level checks cannot detect it"
        );
    }

    #[test]
    fn temporal_check_is_declared_absent_for_a_single_imported_image() {
        // Guards against the temptation to fake a burst by analysing one image
        // repeatedly, which would report a variance of zero as "no glare risk".
        let frame = synthetic_frame(default_spec());
        let png = encode_png(&frame).unwrap();
        let imported = load_from_bytes(&png).unwrap();

        assert!(!imported.info.imported_frame_has_burst);

        let hit = crate::qr::decode(&imported.frame).unwrap().unwrap();
        let l1 = crate::layer1_optical::analyze_burst(
            std::slice::from_ref(&imported.frame),
            (hit.bbox[0], hit.bbox[1], hit.bbox[2], hit.bbox[3]),
        );
        assert_eq!(l1.temporal_glare_var, 0.0);
        assert!(
            l1.warnings.iter().any(|w| w.contains("analisis temporal")),
            "the skipped temporal check must be announced, got {:?}",
            l1.warnings
        );
    }

    #[test]
    fn empty_input_is_rejected() {
        let err = load_from_bytes(&[]).unwrap_err();
        assert!(err.contains("kosong"), "got: {err}");
    }

    #[test]
    fn non_image_bytes_are_rejected_with_a_readable_message() {
        let err = load_from_bytes(b"this is definitely not an image").unwrap_err();
        assert!(
            err.contains("format") || err.contains("dekode"),
            "the message must be actionable, got: {err}"
        );
    }

    #[test]
    fn tiny_image_is_rejected() {
        let tiny = Frame::new(8, 8, vec![0u8; 8 * 8 * 3]).unwrap();
        let png = encode_png(&tiny).unwrap();
        let err = load_from_bytes(&png).unwrap_err();
        assert!(err.contains("terlalu kecil"), "got: {err}");
    }
}
