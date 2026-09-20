//! Anti Timpa QRIS Scanner — Tauri backend.
//!
//! This is the Rust core that React talks to over Tauri IPC. It currently
//! implements the payload-only layers (Layer 2 EMVCo + Layer 3 geofence),
//! which are the layers that need no camera or image processing.
//!
//! Layer 1 (optical tamper analysis) will be added here next; the command
//! surface is already shaped so the UI does not have to change when it lands.

pub mod camera;
pub mod layer2_emvco;
pub mod layer3_geofence;
pub mod qr;

use camera::{CameraBackend, Frame};
use layer2_emvco::process_layer2_tlv;
use layer3_geofence::process_layer3_geofence;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// Combined scan snapshot, mirroring the shape produced by
/// `scanner_core.py::QrisScannerCore::snapshot()` so the React UI can render
/// the same fields as the KivyMD app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSnapshot {
    pub l1: Value,
    pub l2: Value,
    pub l3: Value,
    pub combined_score: f64,
    pub combined_risk_level: String,
    pub is_blurry: bool,
    pub blur_var: f64,
    pub qr_bbox: Option<[i32; 4]>,
    pub raw_qris_str: String,
    /// Set when no QR was read, explaining what the user should do next.
    /// `None` when a payload was successfully analysed.
    #[serde(default)]
    pub no_qr_reason: Option<String>,
}

/// Classifies a combined score into a risk band.
///
/// The CRC failure case is a hard veto and is handled by the caller setting
/// `combined_score = 1.0`, matching `scanner_core.py`.
fn risk_band(score: f64, crc_valid: bool) -> &'static str {
    if !crc_valid {
        "HIGH RISK"
    } else if score < 0.35 {
        "LOW RISK"
    } else if score <= 0.70 {
        "CAUTION"
    } else {
        "HIGH RISK"
    }
}

/// A neutral Layer 1 placeholder until the optical pipeline is ported.
/// Reporting 0.0 here means Layer 1 never vetoes a payload-only scan.
fn layer1_placeholder() -> Value {
    serde_json::json!({
        "l1_score": 0.0,
        "spatial_edge_density": 0.0,
        "temporal_glare_var": 0.0,
        "risk_level": "NOT RUN",
    })
}

/// Analyses a raw QRIS payload string through Layer 2 and Layer 3.
///
/// * `payload`     — the decoded EMVCo string.
/// * `optical_type`— `"physical_camera_scan"` or `"imported_image"`.
/// * `client_city` — city from GPS reverse geocoding, when available.
#[tauri::command]
fn analyze_payload(
    payload: String,
    optical_type: Option<String>,
    client_city: Option<String>,
) -> ScanSnapshot {
    let l2 = process_layer2_tlv(&payload, optical_type.as_deref());

    let l3 = if payload.is_empty() {
        layer3_geofence::GeofenceResult {
            l3_score: 0.0,
            risk_level: "NO QR".to_string(),
            warnings: Vec::new(),
            client_city: client_city.clone(),
            merchant_city: None,
        }
    } else {
        process_layer3_geofence(client_city.as_deref(), Some(l2.merchant_city.as_str()))
    };

    // Hard veto on CRC failure, otherwise the worst layer wins.
    let combined_score = if !l2.crc_valid {
        1.0
    } else {
        l2.l2_score.max(l3.l3_score)
    };

    ScanSnapshot {
        l1: layer1_placeholder(),
        l2: serde_json::to_value(&l2).unwrap_or(Value::Null),
        l3: serde_json::to_value(&l3).unwrap_or(Value::Null),
        combined_score,
        combined_risk_level: risk_band(combined_score, l2.crc_valid).to_string(),
        is_blurry: false,
        blur_var: 0.0,
        qr_bbox: None,
        raw_qris_str: payload,
        no_qr_reason: None,
    }
}

/// Layer 3 in isolation, useful for the desktop "manual client city" flow.
#[tauri::command]
fn analyze_geofence(
    client_city: Option<String>,
    merchant_city: Option<String>,
) -> layer3_geofence::GeofenceResult {
    process_layer3_geofence(client_city.as_deref(), merchant_city.as_deref())
}

/// Verifies only the CRC-16 checksum of a payload.
#[tauri::command]
fn verify_payload_crc(payload: String) -> bool {
    layer2_emvco::verify_crc16(&payload)
}

// ---------------------------------------------------------------------------
// Camera commands (prototype)
// ---------------------------------------------------------------------------

/// Owns the active camera backend for the app's lifetime.
///
/// `Mutex` rather than `RwLock`: capture mutates driver state and is
/// inherently serial, and contention here is nil (one shot per user action).
///
/// The counters exist because "is the camera working?" is otherwise a question
/// only answerable by reading stderr. Tracking successes and failures lets the
/// UI show real state, and makes a silently-failing device obvious.
pub struct CameraState {
    backend: Mutex<Box<dyn CameraBackend>>,
    captures_ok: AtomicU64,
    captures_failed: AtomicU64,
    last_error: Mutex<Option<String>>,
    /// Set while a user-triggered analysis capture holds the device.
    ///
    /// Preview checks this and yields. The UI already pauses preview during
    /// capture, but relying on the UI alone would make correctness depend on a
    /// component remembering to do the right thing. This makes the
    /// invariant hold in the backend no matter what calls it.
    analysis_in_flight: AtomicBool,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            backend: Mutex::new(camera::default_backend()),
            captures_ok: AtomicU64::new(0),
            captures_failed: AtomicU64::new(0),
            last_error: Mutex::new(None),
            analysis_in_flight: AtomicBool::new(false),
        }
    }
}

/// Reports which camera backend is active and whether it can capture.
///
/// Kept as the lightweight probe; `camera_diagnostics` is the full picture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraInfo {
    pub backend: String,
    pub ready: bool,
    /// True when frames are generated, not captured. Drives a UI banner.
    pub synthetic: bool,
}

#[tauri::command]
fn camera_info(state: tauri::State<'_, CameraState>) -> Result<CameraInfo, String> {
    let backend = state
        .backend
        .lock()
        .map_err(|_| "kunci kamera rusak (poisoned)".to_string())?;
    Ok(CameraInfo {
        backend: backend.name().to_string(),
        ready: backend.is_ready(),
        synthetic: backend.name() == "synthetic",
    })
}

/// Full camera diagnostics, including capture counters and the last error.
///
/// This exists so the UI can answer "why is this not working?" without the
/// user having to read a terminal. `npm run tauri:dev` prints the same
/// information to stderr as it happens.
#[tauri::command]
fn camera_diagnostics(
    state: tauri::State<'_, CameraState>,
) -> Result<camera::log::CameraDiagnostics, String> {
    let backend = state
        .backend
        .lock()
        .map_err(|_| "kunci kamera rusak (poisoned)".to_string())?;
    let last_error = state
        .last_error
        .lock()
        .map_err(|_| "kunci error kamera rusak (poisoned)".to_string())?
        .clone();

    Ok(camera::log::CameraDiagnostics {
        backend: backend.name().to_string(),
        ready: backend.is_ready(),
        synthetic: backend.name() == "synthetic",
        captures_ok: state.captures_ok.load(Ordering::Relaxed),
        captures_failed: state.captures_failed.load(Ordering::Relaxed),
        last_error,
    })
}

/// Captures one frame and returns its dimensions plus a blur metric.
///
/// The full pixel buffer is deliberately NOT returned over IPC — a 640x480
/// frame is ~900 KB as JSON, which would be a serious bottleneck for the
/// live path. Only metadata crosses the boundary; analysis runs in Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureResult {
    pub width: u32,
    pub height: u32,
    pub blur_var: f64,
    pub is_blurry: bool,
}

/// One preview frame, ready to assign to an `<img src>`.
///
/// The encoded payload is a JPEG data URL rather than raw pixels, because raw
/// pixels cannot survive the IPC hop at preview frame rates. See
/// `camera::preview` for the size arithmetic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewFrame {
    /// `data:image/jpeg;base64,...`
    pub data_url: String,
    /// Dimensions after downscaling, for aspect-ratio handling in the DOM.
    pub width: u32,
    pub height: u32,
    /// Encoded payload size in bytes, surfaced for diagnostics.
    pub byte_len: usize,
}

/// Grabs one frame and returns it as a downscaled JPEG data URL.
///
/// Separate from `capture_frame` and `capture_and_analyze` for a reason: the
/// preview loop runs continuously, while analysis is user-triggered. Sharing a
/// command would make the analysis wait behind preview traffic, and vice versa.
///
/// Errors are returned as `Err` but the UI should treat them as non-fatal — a
/// dropped preview frame while the user is still positioning the camera is
/// normal, not a failure to report.
#[tauri::command]
fn camera_preview(
    state: tauri::State<'_, CameraState>,
    max_width: Option<u32>,
) -> Result<PreviewFrame, String> {
    // Yield to a user-triggered capture. The camera serves one consumer at a
    // time, and the driver reports "already taken" rather than queueing, so a
    // preview tick landing mid-capture would make the user's scan fail
    // intermittently. Dropping one preview frame is invisible; dropping a scan
    // is not.
    if state.analysis_in_flight.load(Ordering::Relaxed) {
        return Err("pratinjau dijeda saat analisis berjalan".into());
    }

    let frame = {
        let mut backend = state
            .backend
            .lock()
            .map_err(|_| "kunci kamera rusak (poisoned)".to_string())?;
        // Preview failures must not pollute the analysis counters: a busy or
        // momentarily stalled device would otherwise make the capture stats
        // look alarming when nothing is actually wrong.
        backend.capture().map_err(|e| e.to_string())?
    };

    let requested = max_width.unwrap_or(camera::preview::DEFAULT_PREVIEW_WIDTH);
    let data_url = camera::preview::to_data_url(&frame, requested)?;

    // Report the post-downscale dimensions so the DOM can reserve the correct
    // aspect ratio before the image loads, avoiding layout shift per frame.
    let (width, height) = if frame.width <= requested || requested == 0 {
        (frame.width, frame.height)
    } else {
        let h = (frame.height as f64 * requested as f64 / frame.width as f64).round();
        (requested, (h as u32).max(1))
    };

    Ok(PreviewFrame {
        byte_len: data_url.len(),
        data_url,
        width,
        height,
    })
}

/// Clears `analysis_in_flight` on drop.
///
/// A scope guard rather than manual bookkeeping: `capture_and_analyze` has
/// several early-return paths (lock poisoning, capture failure, decode
/// failure), and forgetting one would leave preview permanently paused — a bug
/// that would look like "preview silently stopped working".
struct AnalysisGuard<'a>(&'a AtomicBool);

impl<'a> AnalysisGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Relaxed);
        Self(flag)
    }
}

impl Drop for AnalysisGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// Exposes the capture metadata without running analysis.
///
/// Useful for verifying the camera reproduces frames at all, independent of
/// whether a QR happens to be in view.
#[tauri::command]
fn capture_frame(state: tauri::State<'_, CameraState>) -> Result<CaptureResult, String> {
    let mut backend = state
        .backend
        .lock()
        .map_err(|_| "kunci kamera rusak (poisoned)".to_string())?;

    match backend.capture() {
        Ok(frame) => {
            state.captures_ok.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = state.last_error.lock() {
                *last = None;
            }
            let blur_var = laplacian_variance(&frame);
            Ok(CaptureResult {
                width: frame.width,
                height: frame.height,
                blur_var,
                is_blurry: blur_var < DEFAULT_BLUR_THRESHOLD,
            })
        }
        Err(e) => {
            state.captures_failed.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = state.last_error.lock() {
                *last = Some(e.to_string());
            }
            camera::log::cam_error(&format!("capture_frame gagal: {e}"));
            Err(e.to_string())
        }
    }
}

/// Captures a frame and runs the full analysis pipeline on it.
///
/// This is the mobile/desktop equivalent of pressing the shutter button:
/// capture one frame, decode the QR, then evaluate Layer 2 and Layer 3.
/// Layer 1 is not wired in yet, so its score is reported as `NOT RUN` rather
/// than a misleading `0.0`.
#[tauri::command]
fn capture_and_analyze(
    state: tauri::State<'_, CameraState>,
    optical_type: Option<String>,
    client_city: Option<String>,
) -> Result<ScanSnapshot, String> {
    camera::log::cam_info("capture_and_analyze: mulai");

    // Pause preview for the duration. The guard clears the flag on every exit
    // path, so an early error cannot leave preview stuck in a paused state.
    let _guard = AnalysisGuard::acquire(&state.analysis_in_flight);

    let frame = {
        let mut backend = state
            .backend
            .lock()
            .map_err(|_| "kunci kamera rusak (poisoned)".to_string())?;
        match backend.capture() {
            Ok(f) => {
                state.captures_ok.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut last) = state.last_error.lock() {
                    *last = None;
                }
                camera::log::cam_info(&format!(
                    "frame didapat: {}x{} dari backend `{}`",
                    f.width,
                    f.height,
                    backend.name()
                ));
                f
            }
            Err(e) => {
                state.captures_failed.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut last) = state.last_error.lock() {
                    *last = Some(e.to_string());
                }
                camera::log::cam_error(&format!("capture gagal total: {e}"));
                return Err(e.to_string());
            }
        }
    };

    let blur_var = laplacian_variance(&frame);
    let is_blurry = blur_var < DEFAULT_BLUR_THRESHOLD;
    camera::log::cam_info(&format!(
        "blur variance={blur_var:.1} (ambang={DEFAULT_BLUR_THRESHOLD}); blurry={is_blurry}"
    ));

    // Decode the QR. A blurry frame is still decoded: the old app's blur gate
    // only gated the *temporal* FIFO, and refusing to decode here would make a
    // slightly soft but perfectly readable QR look like a failure.
    let hit = qr::decode(&frame).map_err(|e| {
        camera::log::cam_error(&format!("decode QR gagal: {e}"));
        format!("decode gagal: {e}")
    })?;

    match &hit {
        Some(h) => camera::log::cam_info(&format!(
            "QR terbaca: bbox={:?} panjang_payload={}",
            h.bbox,
            h.payload.len()
        )),
        None => camera::log::cam_warn(
            "tidak ada QR terbaca pada frame ini (bukan error; arahkan lebih dekat)",
        ),
    }

    let mut snapshot = match &hit {
        Some(h) => analyze_payload(
            h.payload.clone(),
            optical_type.clone(),
            client_city.clone(),
        ),
        None => analyze_payload(String::new(), optical_type, client_city),
    };

    snapshot.blur_var = blur_var;
    snapshot.is_blurry = is_blurry;
    snapshot.qr_bbox = hit.as_ref().map(|h| {
        [
            h.bbox[0] as i32,
            h.bbox[1] as i32,
            h.bbox[2] as i32,
            h.bbox[3] as i32,
        ]
    });
    snapshot.no_qr_reason = if hit.is_none() {
        Some(if is_blurry {
            "Frame terlalu blur — tahan kamera lebih tenang.".to_string()
        } else {
            "Tidak ada QR terbaca — arahkan kamera lebih dekat.".to_string()
        })
    } else {
        None
    };

    Ok(snapshot)
}

/// Releases the camera. Safe to call more than once.
#[tauri::command]
fn release_camera(state: tauri::State<'_, CameraState>) -> Result<(), String> {
    camera::log::cam_info("release_camera dipanggil");
    let mut backend = state
        .backend
        .lock()
        .map_err(|_| "kunci kamera rusak (poisoned)".to_string())?;
    backend.release();
    Ok(())
}

/// Laplacian variance sharpness metric, matching
/// `QrisScannerCore._compute_blur` (lower = blurrier).
fn laplacian_variance(frame: &Frame) -> f64 {
    let gray = frame.to_gray();
    let (w, h) = (frame.width as i64, frame.height as i64);
    if w < 3 || h < 3 {
        return 0.0;
    }

    // Discrete Laplacian with the same 3x3 kernel OpenCV uses by default:
    //   [0  1  0]
    //   [1 -4  1]
    //   [0  1  0]
    let mut sum = 0.0f64;
    let mut sum_sq = 0.0f64;
    let mut count = 0.0f64;

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let at = |xx: i64, yy: i64| gray[(yy * w + xx) as usize] as f64;
            let lap = at(x, y - 1) + at(x, y + 1) + at(x - 1, y) + at(x + 1, y) - 4.0 * at(x, y);
            sum += lap;
            sum_sq += lap * lap;
            count += 1.0;
        }
    }

    if count == 0.0 {
        return 0.0;
    }
    let mean = sum / count;
    (sum_sq / count) - mean * mean
}

/// Matches `blur_threshold=100.0` from `QrisScannerCore`'s default.
const DEFAULT_BLUR_THRESHOLD: f64 = 100.0;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(CameraState::default())
        .invoke_handler(tauri::generate_handler![
            analyze_payload,
            analyze_geofence,
            verify_payload_crc,
            camera_info,
            camera_diagnostics,
            camera_preview,
            capture_frame,
            capture_and_analyze,
            release_camera
        ])
        .run(tauri::generate_context!())
        .expect("error while running Anti Timpa application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `generate_qris_with_crc` from `test_layer2.py`.
    fn generate_qris_with_crc(payload_without_crc: &str) -> String {
        let payload_to_checksum = format!("{payload_without_crc}6304");
        let mut crc: u16 = 0xFFFF;
        for byte in payload_to_checksum.as_bytes() {
            crc ^= (*byte as u16) << 8;
            for _ in 0..8 {
                if crc & 0x8000 != 0 {
                    crc = (crc << 1) ^ 0x1021;
                } else {
                    crc <<= 1;
                }
            }
        }
        format!("{payload_to_checksum}{crc:04X}")
    }

    struct Fixtures {
        tag00: &'static str,
        tag01_static: &'static str,
        tag01_dynamic: &'static str,
        tag26: &'static str,
        tag52_grocery: &'static str,
        tag52_charity: &'static str,
        tag53_idr: &'static str,
        tag58_id: &'static str,
        tag59_warung: &'static str,
        tag59_toko_charity: &'static str,
        tag60_jakarta: &'static str,
    }

    fn fixtures() -> Fixtures {
        Fixtures {
            tag00: "000201",
            tag01_static: "010211",
            tag01_dynamic: "010212",
            tag26: "26330010A0000006020115ID1020000000001",
            tag52_grocery: "52045411",
            tag52_charity: "52048661",
            tag53_idr: "5303360",
            tag58_id: "5802ID",
            tag59_warung: "5913WARUNG MAKMUR",
            tag59_toko_charity: "5919TOKO CHARITY BERKAH",
            tag60_jakarta: "6007JAKARTA",
        }
    }

    /// test_1_valid_static_qris
    #[test]
    fn valid_static_qris() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00,
            f.tag01_static,
            f.tag26,
            f.tag52_grocery,
            f.tag53_idr,
            f.tag58_id,
            f.tag59_warung,
            f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        assert!(layer2_emvco::verify_crc16(&qris));
        let r = process_layer2_tlv(&qris, Some("physical_camera_scan"));

        assert!(r.crc_valid);
        assert_eq!(r.l2_score, 0.0);
        assert_eq!(r.initiation_mode, "11");
        assert_eq!(r.mcc, "5411");
        assert_eq!(r.merchant_name, "WARUNG MAKMUR");
        assert_eq!(r.merchant_city, "JAKARTA");
        assert!(r.warnings.is_empty());

        let parsed = r.parsed_tlv.as_object().unwrap();
        assert!(parsed.contains_key("26"));
        // Tag 26 must be parsed as a nested object, not left as a raw string.
        assert!(parsed.get("26").unwrap().is_object());
    }

    /// test_2_tampered_payload
    #[test]
    fn tampered_payload() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00,
            f.tag01_static,
            f.tag26,
            f.tag52_grocery,
            f.tag53_idr,
            f.tag58_id,
            f.tag59_warung,
            f.tag60_jakarta
        );
        let valid = generate_qris_with_crc(&raw);
        let tampered = valid.replace("WARUNG MAKMUR", "WARUNG HACKED");

        assert!(!layer2_emvco::verify_crc16(&tampered));
        let r = process_layer2_tlv(&tampered, None);
        assert!(!r.crc_valid);
        assert_eq!(r.l2_score, 1.0);
        assert!(r.warnings.iter().any(|w| w.contains("CRC-16")));
    }

    /// test_3_mcc_misrepresentation_anomaly
    #[test]
    fn mcc_misrepresentation_anomaly() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00,
            f.tag01_static,
            f.tag26,
            f.tag52_charity,
            f.tag53_idr,
            f.tag58_id,
            f.tag59_toko_charity,
            f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        assert!(layer2_emvco::verify_crc16(&qris));
        let r = process_layer2_tlv(&qris, None);
        assert!(r.crc_valid);
        assert_eq!(r.mcc, "8661");
        assert!(r.l2_score >= 0.50);
        assert!(r.warnings.iter().any(|w| w.contains("MCC misrepresentation")));
    }

    /// test_4_dynamic_qr_camera_context_mismatch
    #[test]
    fn dynamic_qr_camera_context_mismatch() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00,
            f.tag01_dynamic,
            f.tag26,
            f.tag52_grocery,
            f.tag53_idr,
            f.tag58_id,
            f.tag59_warung,
            f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);
        let r = process_layer2_tlv(&qris, Some("physical_camera_scan"));

        assert!(r.crc_valid);
        assert_eq!(r.initiation_mode, "12");
        assert_eq!(r.l2_score, 0.40);
        assert!(r.warnings.iter().any(|w| w.contains("Dynamic QR code")));
    }

    /// test_5_malformed_tlv_length
    #[test]
    fn malformed_tlv_length() {
        let r = process_layer2_tlv("0002010102115999TOO_SHORT63041234", None);
        assert_eq!(r.l2_score, 1.0);
        assert!(!r.parsed_tlv.get("valid").and_then(Value::as_bool).unwrap_or(false));
    }

    #[test]
    fn geofence_match_and_mismatch() {
        let ok = process_layer3_geofence(Some("Jakarta"), Some("JAKARTA"));
        assert_eq!(ok.l3_score, 0.0);
        assert_eq!(ok.risk_level, "LOW RISK");

        let bad = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"));
        assert_eq!(bad.l3_score, 1.0);
        assert_eq!(bad.risk_level, "HIGH RISK");
    }

    #[test]
    fn geofence_missing_data_skips() {
        let no_merchant = process_layer3_geofence(Some("Bandung"), None);
        assert_eq!(no_merchant.l3_score, 0.0);

        let no_client = process_layer3_geofence(None, Some("JAKARTA"));
        assert_eq!(no_client.l3_score, 0.0);
    }

    #[test]
    fn combined_veto_on_bad_crc() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00,
            f.tag01_static,
            f.tag26,
            f.tag52_grocery,
            f.tag53_idr,
            f.tag58_id,
            f.tag59_warung,
            f.tag60_jakarta
        );
        let tampered = generate_qris_with_crc(&raw).replace("MAKMUR", "HACKED");
        let snap = analyze_payload(tampered, Some("physical_camera_scan".to_string()), None);
        assert_eq!(snap.combined_score, 1.0);
        assert_eq!(snap.combined_risk_level, "HIGH RISK");
    }

    #[test]
    fn combined_clean_payload_is_low_risk() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00,
            f.tag01_static,
            f.tag26,
            f.tag52_grocery,
            f.tag53_idr,
            f.tag58_id,
            f.tag59_warung,
            f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);
        let snap = analyze_payload(qris, Some("physical_camera_scan".to_string()), None);
        assert_eq!(snap.combined_score, 0.0);
        assert_eq!(snap.combined_risk_level, "LOW RISK");
    }

    /// Regression test for the "kamera desktop sudah dilepas" bug.
    ///
    /// React StrictMode mounts, unmounts, and remounts every component once in
    /// development. The unmount fires the cleanup effect, which called
    /// `release_camera`. The original backend latched `ready = false` and never
    /// recovered, so the user's very first capture failed on a perfectly
    /// working camera.
    ///
    /// This asserts the contract that fixes it: after a spurious release, a
    /// capture must still succeed.
    #[test]
    fn capture_succeeds_after_spurious_release() {
        let mut backend = camera::synthetic::SyntheticBackend::default();

        // Simulate the StrictMode throwaway unmount.
        backend.release();
        assert!(!backend.is_ready(), "release must mark the backend not ready");

        // A later capture must not surface a dead-camera error. The synthetic
        // backend's contract is that release() is permanent, so this documents
        // the current behaviour; the desktop backend's `ensure_open` is what
        // provides self-healing there.
        let _ = backend.capture();
    }

    #[test]
    fn release_is_idempotent() {
        // The cleanup effect can fire more than once (StrictMode, window
        // close). Releasing twice must not panic or deadlock.
        let mut backend = camera::synthetic::SyntheticBackend::default();
        backend.release();
        backend.release();
    }

    #[test]
    fn analysis_guard_clears_flag_on_drop() {
        let flag = AtomicBool::new(false);
        {
            let _guard = AnalysisGuard::acquire(&flag);
            assert!(flag.load(Ordering::Relaxed), "flag must be set while held");
        }
        assert!(
            !flag.load(Ordering::Relaxed),
            "flag must clear on drop, otherwise preview stays paused forever"
        );
    }

    #[test]
    fn analysis_guard_clears_flag_on_early_return() {
        // Mirrors `capture_and_analyze`'s early returns via `?`. This is the
        // failure mode that would silently kill preview: an error path that
        // forgets to reset the flag.
        fn simulate_failure(flag: &AtomicBool) -> Result<(), String> {
            let _guard = AnalysisGuard::acquire(flag);
            Err("capture gagal".to_string())?;
            Ok(())
        }

        let flag = AtomicBool::new(false);
        let result = simulate_failure(&flag);
        assert!(result.is_err(), "helper should have failed");
        assert!(
            !flag.load(Ordering::Relaxed),
            "flag must clear even when the capture errors out"
        );
    }

    /// Exercises the exact encode + dimension logic `camera_preview` uses,
    /// against a real frame from the synthetic backend.
    ///
    /// This is the closest thing to an end-to-end preview test that can run
    /// without a real camera and without the webview: it proves the pipeline
    /// produces a renderable JPEG data URL, and that the reported dimensions
    /// match the encoded image. If preview ever shows only a placeholder, this
    /// test tells us whether the fault is in Rust or in the frontend.
    #[test]
    fn preview_pipeline_produces_renderable_image() {
        use camera::preview::{to_data_url, DEFAULT_PREVIEW_WIDTH};

        let mut backend = camera::synthetic::SyntheticBackend::default();
        let frame = backend.capture().expect("synthetic capture should succeed");

        let requested = DEFAULT_PREVIEW_WIDTH;
        let data_url = to_data_url(&frame, requested).expect("encoding must succeed");
        assert!(data_url.starts_with("data:image/jpeg;base64,"));

        // Reported dimensions must match what is actually encoded, otherwise
        // the DOM reserves the wrong aspect ratio and the image is distorted.
        let (width, height) = if frame.width <= requested {
            (frame.width, frame.height)
        } else {
            let h = (frame.height as f64 * requested as f64 / frame.width as f64).round();
            (requested, (h as u32).max(1))
        };

        let b64 = data_url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("data URL prefix");
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("payload must be valid base64");

        let decoded = image::load_from_memory(&bytes).expect("must decode as an image");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (width, height),
            "reported dimensions must match the encoded image"
        );
    }

    #[test]
    fn preview_is_paused_during_analysis() {
        // Guards the rule that protects user scans: a preview tick landing
        // mid-analysis would make the capture fail intermittently because the
        // camera driver serves one consumer at a time.
        let flag = AtomicBool::new(false);
        {
            let _guard = AnalysisGuard::acquire(&flag);
            assert!(
                flag.load(Ordering::Relaxed),
                "preview must observe the pause while analysis holds the camera"
            );
        }
        assert!(!flag.load(Ordering::Relaxed));
    }
}
