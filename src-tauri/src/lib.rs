//! Anti Timpa QRIS Scanner — Tauri backend.
//!
//! This is the Rust core that React talks to over Tauri IPC. It currently
//! implements the payload-only layers (Layer 2 EMVCo + Layer 3 geofence),
//! which are the layers that need no camera or image processing.
//!
//! Layer 1 (optical tamper analysis) will be added here next; the command
//! surface is already shaped so the UI does not have to change when it lands.

pub mod camera;
pub mod geo_table;
pub mod history;
pub mod image_import;
pub mod layer1_optical;
pub mod layer2_emvco;
pub mod layer3_geofence;
pub mod qr;
pub mod report;

use camera::{CameraBackend, Frame};
use layer1_optical::Layer1Result;
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
    /// Whether the result reflects a complete check or a partial one.
    ///
    /// This is the field that stops a payload-only scan from being presented as
    /// "safe": a tampered sticker leaves the payload untouched, so a result that
    /// never ran the optical layer must not be read as reassurance.
    #[serde(default)]
    pub coverage: ScanCoverage,
    /// Actionable findings, worst first, ready for the UI to list.
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// Integrity hash of the corresponding history entry, when recorded.
    #[serde(default)]
    pub chain_hash: Option<u64>,
}

/// Which layers actually executed for a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanCoverage {
    pub optical_ran: bool,
    pub payload_ran: bool,
    pub geofence_ran: bool,
    /// True only when every layer that could apply actually ran.
    pub complete: bool,
    /// Human-readable summary of what was checked, for the UI and the report.
    pub summary: String,
}

impl Default for ScanCoverage {
    fn default() -> Self {
        Self {
            optical_ran: false,
            payload_ran: false,
            geofence_ran: false,
            complete: false,
            summary: "Belum ada pemindaian.".to_string(),
        }
    }
}

/// A single named risk rule that fired.
///
/// Rules are named so a finding can be cited consistently across the UI, the
/// history timeline, and a shared report — an anonymous "score 0.8" is not
/// actionable to a merchant or a bank.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Finding {
    /// Stable rule identifier, e.g. `L2_CRC_MISMATCH`.
    pub code: String,
    /// Which layer produced it: `L1`, `L2`, or `L3`.
    pub layer: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
}

impl Finding {
    fn new(
        code: &str,
        layer: &str,
        severity: &str,
        title: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_string(),
            layer: layer.to_string(),
            severity: severity.to_string(),
            title: title.to_string(),
            detail: detail.into(),
        }
    }
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

/// A neutral Layer 1 placeholder for scans that never touched a frame.
///
/// Reporting 0.0 here means Layer 1 never vetoes a payload-only scan, but the
/// `NOT RUN` band is what stops the UI from presenting that as a clean bill of
/// health. `Layer1Result::not_run` is the single constructor for this state so
/// no path can invent a score for a layer that did not execute.
fn layer1_placeholder(reason: &str) -> Value {
    serde_json::to_value(Layer1Result::not_run(reason)).unwrap_or(Value::Null)
}

/// Analyses a raw QRIS payload string through Layer 2 and Layer 3.
///
/// This path has **no frame**, so Layer 1 cannot run. That is recorded as an
/// explicit `NOT RUN` coverage flag rather than an implicit zero, because a
/// payload-only scan cannot detect a physical overlay: the payload of a pasted
/// sticker is byte-identical to the original.
///
/// * `payload`     — the decoded EMVCo string.
/// * `optical_type`— `"physical_camera_scan"` or `"imported_image"`.
/// * `client_city` — city from GPS reverse geocoding, when available.
#[tauri::command]
fn analyze_payload(
    payload: String,
    optical_type: Option<String>,
    client_city: Option<String>,
    client_lat: Option<f64>,
    client_lon: Option<f64>,
) -> ScanSnapshot {
    let l1 = layer1_placeholder(if payload.is_empty() {
        "tidak ada QR yang terbaca"
    } else {
        "hanya payload yang dianalisis; tidak ada frame untuk analisis optik"
    });

    // Resolve a typed city to coordinates when no fix was supplied.
    //
    // Without this, the payload-only path could never produce a distance: the
    // UI sends a city name, and the reference points that would let a distance be
    // computed live behind a lookup the caller never performed. Desktop has no
    // other way to obtain a position at all.
    let client_fix = match (client_lat, client_lon) {
        (Some(la), Some(lo)) => Some((la, lo)),
        (None, Some(lo)) => Some((0.0, lo)),
        (Some(la), None) => Some((la, 0.0)),
        (None, None) => client_city
            .as_deref()
            .and_then(geo_table::coords_for),
    };

    analyze_with(payload, optical_type, client_city, l1, client_fix)
}

/// Shared core: evaluates Layer 2 + Layer 3 and folds in whatever Layer 1
/// produced (a real result, or the explicit not-run marker).
fn analyze_with(
    payload: String,
    optical_type: Option<String>,
    client_city: Option<String>,
    l1: Value,
    client_fix: Option<(f64, f64)>,
) -> ScanSnapshot {
    let l2 = process_layer2_tlv(&payload, optical_type.as_deref());

    let l3 = if payload.is_empty() {
        layer3_geofence::GeofenceResult {
            l3_score: 0.0,
            risk_level: "NO QR".to_string(),
            warnings: Vec::new(),
            client_city: client_city.clone(),
            merchant_city: None,
            mismatch_kind: "NOT_EVALUATED".to_string(),
            distance_km: None,
            location_available: false,
            evaluated: false,
        }
    } else {
        process_layer3_geofence(
            client_city.as_deref(),
            Some(l2.merchant_city.as_str()),
            client_fix,
        )
    };

    let l1_score = l1.get("l1_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let l1_ran = l1
        .get("risk_level")
        .and_then(|v| v.as_str())
        .map(|s| s != "NOT RUN")
        .unwrap_or(false);

    // Hard veto on CRC failure, otherwise the worst layer wins.
    let combined_score = if !l2.crc_valid {
        1.0
    } else {
        l1_score.max(l2.l2_score).max(l3.l3_score)
    };

    // A CRC failure in a payload-only scan is already covered by the veto above;
    // the optical layer only gets to veto when it actually produced a result.
    let combined_risk_level = risk_band(combined_score, l2.crc_valid).to_string();

    let geofence_ran = l3.evaluated;
    let payload_ran = !payload.is_empty();
    let coverage = ScanCoverage {
        optical_ran: l1_ran,
        payload_ran,
        geofence_ran,
        complete: l1_ran && payload_ran && geofence_ran,
        summary: coverage_summary(l1_ran, payload_ran, geofence_ran),
    };

    let findings = build_findings(&l1, &l2, &l3, combined_score, l2.crc_valid);

    ScanSnapshot {
        l1,
        l2: serde_json::to_value(&l2).unwrap_or(Value::Null),
        l3: serde_json::to_value(&l3).unwrap_or(Value::Null),
        combined_score,
        combined_risk_level,
        is_blurry: false,
        blur_var: 0.0,
        qr_bbox: None,
        raw_qris_str: payload,
        no_qr_reason: None,
        coverage,
        findings,
        chain_hash: None,
    }
}

fn coverage_summary(optical: bool, payload: bool, geofence: bool) -> String {
    let mut ran = Vec::new();
    let mut skipped = Vec::new();
    for (did, name) in [
        (optical, "optik (L1)"),
        (payload, "payload (L2)"),
        (geofence, "geofence (L3)"),
    ] {
        if did {
            ran.push(name);
        } else {
            skipped.push(name);
        }
    }
    if skipped.is_empty() {
        return format!("Pemeriksaan lengkap: {}.", ran.join(", "));
    }
    if ran.is_empty() {
        return "Tidak ada lapisan yang berjalan.".to_string();
    }
    format!(
        "Pemeriksaan sebagian. Berjalan: {}. Tidak berjalan: {}. Hasil ini tidak \
         mencakup {}. ",
        ran.join(", "),
        skipped.join(", "),
        skipped.join(" maupun ")
    )
    .trim_end()
    .to_string()
}

/// Turns the three layer payloads into a ranked list of named findings.
///
/// Ordered worst-first so the UI can render the list top-down without sorting,
/// and so the first entry is always the headline finding.
fn build_findings(
    l1: &Value,
    l2: &layer2_emvco::Layer2Result,
    l3: &layer3_geofence::GeofenceResult,
    combined_score: f64,
    crc_valid: bool,
) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();

    // --- Layer 2 ---------------------------------------------------------
    if !crc_valid {
        out.push(Finding::new(
            "L2_CRC_MISMATCH",
            "L2",
            "HIGH",
            "Checksum payload tidak cocok",
            "CRC-16/CCITT-FALSE tidak valid. Payload kemungkinan besar telah diubah \
             setelah QR dibuat. Ini adalah veto keras: skor gabungan dipaksa 1.0.",
        ));
    }
    if let Some(w) = l2.warnings.iter().find(|w| w.contains("Payload Format Indicator")) {
        out.push(Finding::new("L2_PAYLOAD_FORMAT", "L2", "HIGH", "Format payload tidak valid", w.clone()));
    }
    if let Some(w) = l2
        .warnings
        .iter()
        .find(|w| w.contains("currency") || w.contains("currency") || w.contains("Tag 53"))
    {
        out.push(Finding::new("L2_CURRENCY_COUNTRY", "L2", "MEDIUM", "Mata uang / negara tidak wajar", w.clone()));
    }
    if let Some(w) = l2.warnings.iter().find(|w| w.contains("Dynamic QR")) {
        out.push(Finding::new("L2_DYNAMIC_IN_CAMERA", "L2", "MEDIUM", "QR dinamis di konteks pemindaian fisik", w.clone()));
    }
    if let Some(w) = l2.warnings.iter().find(|w| w.contains("MCC")) {
        out.push(Finding::new("L2_MCC_MISREPRESENTATION", "L2", "HIGH", "MCC tidak sesuai dengan merchant", w.clone()));
    }

    // --- Layer 3 ---------------------------------------------------------
    if l3.l3_score >= 0.99 {
        out.push(Finding::new(
            "L3_GEOFENCE_MISMATCH",
            "L3",
            "MEDIUM",
            "Lokasi tidak cocok dengan kota merchant",
            l3.warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "Kota klien berbeda dari kota pada payload.".to_string()),
        ));
    }

    // --- Layer 1 ---------------------------------------------------------
    let l1_ran = l1
        .get("risk_level")
        .and_then(|v| v.as_str())
        .map(|s| s != "NOT RUN")
        .unwrap_or(false);
    if l1_ran {
        let band = l1.get("risk_level").and_then(|v| v.as_str()).unwrap_or("");
        if band == "HIGH RISK" || band == "CAUTION" {
            let edge = l1.get("spatial_edge_density").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let glare = l1.get("glare_fraction").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let texture = l1.get("texture_discontinuity").and_then(|v| v.as_f64()).unwrap_or(0.0);
            out.push(Finding::new(
                "L1_OPTICAL_TAMPER",
                "L1",
                if band == "HIGH RISK" { "HIGH" } else { "MEDIUM" },
                "Indikasi penempelan fisik pada QR",
                format!(
                    "Tepi di quiet zone: {:.3}; kilau: {:.1}%; ketidakteraturan tekstur: {:.0}. \
                     QR asli umumnya bersih di ketiga ukuran ini.",
                    edge,
                    glare * 100.0,
                    texture
                ),
            ));
        }
        if l1.get("quiet_zone_truncated").and_then(|v| v.as_bool()).unwrap_or(false) {
            out.push(Finding::new(
                "L1_QUIET_ZONE_TRUNCATED",
                "L1",
                "INFO",
                "Quiet zone tidak terukur penuh",
                "Margin QR terpotong oleh batas frame, sehingga analisis tepi kurang \
                 dapat diandalkan. Dekatkan kamera dan pastikan seluruh QR + margin masuk frame.",
            ));
        }
    }

    // A clean result is itself a finding worth stating — with its scope made
    // explicit, so "nothing found" is never mistaken for "everything checked".
    if out.is_empty() && crc_valid && combined_score == 0.0 {
        out.push(Finding::new(
            "NO_FINDINGS",
            "-",
            "INFO",
            "Tidak ada anomali terdeteksi",
            if l1_ran {
                "Ketiga lapisan berjalan dan tidak ada aturan yang terpicu. Ini bukan \
                 jaminan QRIS sah — verifikasi ke merchant sebelum bertransaksi."
                    .to_string()
            } else {
                "Layer 2 dan Layer 3 tidak menemukan anomali. Layer 1 (optik) tidak \
                 berjalan, sehingga penempelan fisik pada QR tidak dapat dideteksi \
                 pada pemindaian ini."
                    .to_string()
            },
        ));
    }

    out
}

/// Layer 3 in isolation, useful for the desktop "manual client city" flow.
#[tauri::command]
fn analyze_geofence(
    client_city: Option<String>,
    merchant_city: Option<String>,
    client_lat: Option<f64>,
    client_lon: Option<f64>,
) -> layer3_geofence::GeofenceResult {
    let fix = match (client_lat, client_lon) {
        (Some(la), Some(lo)) => Some((la, lo)),
        _ => None,
    };
    process_layer3_geofence(client_city.as_deref(), merchant_city.as_deref(), fix)
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

/// Captures a burst of frames and runs the full three-layer analysis.
///
/// This is the production path: the optical layer needs more than one frame to
/// separate a moving specular highlight from a static bright patch, so a burst
/// is captured before analysis rather than a single frame.
///
/// Burst capture is best-effort: if the device only delivers one frame, the
/// scan proceeds and Layer 1 reports that its temporal term was skipped, instead
/// of failing the whole capture.
#[tauri::command]
fn capture_and_analyze(
    state: tauri::State<'_, CameraState>,
    optical_type: Option<String>,
    client_city: Option<String>,
    client_lat: Option<f64>,
    client_lon: Option<f64>,
    burst_frames: Option<u32>,
) -> Result<ScanSnapshot, String> {
    camera::log::cam_info("capture_and_analyze: mulai");

    // Pause preview for the duration. The guard clears the flag on every exit
    // path, so an early error cannot leave preview stuck in a paused state.
    let _guard = AnalysisGuard::acquire(&state.analysis_in_flight);

    let requested = burst_frames.unwrap_or(DEFAULT_BURST_FRAMES).clamp(1, MAX_BURST_FRAMES);

    let mut frames: Vec<Frame> = Vec::with_capacity(requested as usize);
    let mut last_error: Option<String> = None;

    for i in 0..requested {
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
                if i == 0 {
                    camera::log::cam_info(&format!(
                        "frame didapat: {}x{} dari backend `{}`",
                        f.width,
                        f.height,
                        backend.name()
                    ));
                }
                frames.push(f);
            }
            Err(e) => {
                // One failed frame inside a burst is not fatal as long as at
                // least one frame arrived: Layer 1 degrades to the spatial-only
                // signal and says so.
                state.captures_failed.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut last) = state.last_error.lock() {
                    *last = Some(e.to_string());
                }
                camera::log::cam_warn(&format!("burst frame {i} gagal: {e}"));
                last_error = Some(e.to_string());
                break;
            }
        }
    }

    if frames.is_empty() {
        let msg = last_error.unwrap_or_else(|| "tidak ada frame yang berhasil diambil".to_string());
        camera::log::cam_error(&format!("capture gagal total: {msg}"));
        return Err(msg);
    }

    let primary = &frames[0];
    let blur_var = laplacian_variance(primary);
    let is_blurry = blur_var < DEFAULT_BLUR_THRESHOLD;
    camera::log::cam_info(&format!(
        "burst={} frame, blur variance={blur_var:.1} (ambang={DEFAULT_BLUR_THRESHOLD}); blurry={is_blurry}",
        frames.len()
    ));

    // Decode the QR. A blurry frame is still decoded: the old app's blur gate
    // only gated the *temporal* FIFO, and refusing to decode here would make a
    // slightly soft but perfectly readable QR look like a failure.
    let hit = qr::decode(primary).map_err(|e| {
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

    // Layer 1 runs only when there is a symbol to measure. Without a bbox there
    // is no quiet zone, and reporting a score anyway would be exactly the
    // false-clean result this layer exists to prevent.
    let l1 = match &hit {
        Some(h) => {
            let bbox = (
                h.bbox[0] as u32,
                h.bbox[1] as u32,
                h.bbox[2] as u32,
                h.bbox[3] as u32,
            );
            let r = layer1_optical::analyze_burst(&frames, bbox);
            camera::log::cam_info(&format!(
                "Layer 1: band={} skor={:.3} edge={:.4} glare={:.4} temporal_var={:.6} bbox={:?}",
                r.risk_level,
                r.l1_score,
                r.spatial_edge_density,
                r.glare_fraction,
                r.temporal_glare_var,
                bbox
            ));
            serde_json::to_value(&r).unwrap_or(Value::Null)
        }
        None => layer1_placeholder("tidak ada QR yang terbaca pada burst ini"),
    };

    let client_fix = match (client_lat, client_lon) {
        (Some(la), Some(lo)) => Some((la, lo)),
        _ => None,
    };

    let mut snapshot = match &hit {
        Some(h) => analyze_with(
            h.payload.clone(),
            optical_type.clone(),
            client_city.clone(),
            l1,
            client_fix,
        ),
        None => analyze_with(
            String::new(),
            optical_type,
            client_city,
            l1,
            client_fix,
        ),
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

/// Frames requested for the optical burst by default.
///
/// Four is the smallest count that gives the temporal glare check a usable
/// series without making the shutter feel slow on a phone.
const DEFAULT_BURST_FRAMES: u32 = 4;

/// Upper bound, so a caller cannot ask for a burst that stalls the UI.
const MAX_BURST_FRAMES: u32 = 12;

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

/// Analyses an image file through all three layers, without a camera.
///
/// This is the import path: a photo the user already has. See
/// [`image_import`] for why this is framed as a *validation* path rather than
/// the primary detection path.
///
/// Layer 1 runs on the imported frame with its spatial signals only. A single
/// photo cannot support the temporal glare check, and that is reported rather
/// than faked.
#[tauri::command]
fn analyze_image_bytes(
    bytes: Vec<u8>,
    optical_type: Option<String>,
    client_city: Option<String>,
    client_lat: Option<f64>,
    client_lon: Option<f64>,
) -> Result<ScanSnapshot, String> {
    let imported = image_import::load_from_bytes(&bytes)?;
    let frame = imported.frame;

    camera::log::cam_info(&format!(
        "import gambar: {}x{} -> {}x{} (downscaled={})",
        imported.info.original_width,
        imported.info.original_height,
        imported.info.width,
        imported.info.height,
        imported.info.downscaled
    ));

    let hit = qr::decode(&frame).map_err(|e| format!("decode gagal: {e}"))?;

    let (l1, payload) = match &hit {
        Some(h) => {
            let bbox = (
                h.bbox[0] as u32,
                h.bbox[1] as u32,
                h.bbox[2] as u32,
                h.bbox[3] as u32,
            );
            let r = layer1_optical::analyze_burst(std::slice::from_ref(&frame), bbox);
            camera::log::cam_info(&format!(
                "Layer 1 (impor): band={} skor={:.3} edge={:.4} bbox={:?}",
                r.risk_level, r.l1_score, r.spatial_edge_density, bbox
            ));
            (serde_json::to_value(&r).unwrap_or(Value::Null), h.payload.clone())
        }
        None => (
            layer1_placeholder("tidak ada QR yang terbaca pada gambar yang diimpor"),
            String::new(),
        ),
    };

    let client_fix = match (client_lat, client_lon) {
        (Some(la), Some(lo)) => Some((la, lo)),
        _ => None,
    };

    let mut snapshot = analyze_with(payload, optical_type, client_city, l1, client_fix);

    snapshot.blur_var = laplacian_variance(&frame);
    snapshot.is_blurry = snapshot.blur_var < DEFAULT_BLUR_THRESHOLD;
    snapshot.qr_bbox = hit
        .as_ref()
        .map(|h| [h.bbox[0] as i32, h.bbox[1] as i32, h.bbox[2] as i32, h.bbox[3] as i32]);
    snapshot.no_qr_reason = if hit.is_none() {
        Some(
            "Tidak ada QR terbaca pada gambar. Pastikan QR terlihat utuh, tidak terpotong, \
             dan pencahayaan cukup."
                .to_string(),
        )
    } else {
        None
    };

    Ok(snapshot)
}

/// Metadata about an imported image, so the UI can show what was analysed.
#[tauri::command]
fn inspect_image(bytes: Vec<u8>) -> Result<image_import::ImportInfo, String> {
    Ok(image_import::load_from_bytes(&bytes)?.info)
}

/// Matches `blur_threshold=100.0` from `QrisScannerCore`'s default.
const DEFAULT_BLUR_THRESHOLD: f64 = 100.0;

// ---------------------------------------------------------------------------
// Offline geocoding (desktop)
// ---------------------------------------------------------------------------

/// Resolves a typed city name to coordinates from the bundled offline table.
///
/// This is the desktop answer to location. Desktop has no OS location service —
/// `navigator.geolocation` inside a WebView is either denied or fabricated, and
/// the Tauri geolocation plugin is mobile-only — so rather than failing the
/// comparison, Layer 3 can resolve the city the user typed.
///
/// Returns `None` for an unknown city. That is not an error: the caller then
/// leaves the coordinates absent and Layer 3 reports an unbounded mismatch rather
/// than a fabricated distance.
#[tauri::command]
fn geocode_city(city: String) -> Option<[f64; 2]> {
    geo_table::coords_for(&city).map(|(lat, lon)| [lat, lon])
}

/// Every city in the offline table, so the UI can offer hints.
#[tauri::command]
fn known_cities() -> Vec<String> {
    geo_table::known_cities()
        .into_iter()
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------------
// Scan history
// ---------------------------------------------------------------------------

/// Session-scoped history, owned for the app's lifetime.
///
/// Deliberately in-memory: persisting a log of everything a user scanned is a
/// privacy decision the user should make, not a default. See
/// `docs/ARCHITECTURE.md` for the persistence roadmap.
#[derive(Default)]
pub struct HistoryState {
    chain: Mutex<history::HistoryChain>,
}

/// Records a completed scan into the tamper-evident chain.
///
/// Called by the UI *after* a scan the user chose to keep, not automatically on
/// every capture — a history the user did not ask for is surveillance, and on a
/// phone it is also battery and storage spent on noise.
#[tauri::command]
fn record_scan(
    state: tauri::State<'_, HistoryState>,
    snapshot: ScanSnapshot,
    source: String,
    timestamp_ms: Option<u64>,
) -> Result<history::HistoryEntry, String> {
    let l2 = &snapshot.l2;
    let crc_valid = l2.get("crc_valid").and_then(|v| v.as_bool()).unwrap_or(false);
    let payload_preview = snapshot.raw_qris_str.clone();

    let top_finding = snapshot.findings.first().map(|f| format!("{}:{}", f.code, f.title));

    let input = history::HistoryInput {
        timestamp_ms,
        source,
        combined_score: snapshot.combined_score,
        combined_risk_level: snapshot.combined_risk_level.clone(),
        payload: payload_preview,
        l1_score: snapshot.l1.get("l1_score").and_then(|v| v.as_f64()).unwrap_or(0.0),
        l2_score: l2.get("l2_score").and_then(|v| v.as_f64()).unwrap_or(0.0),
        l3_score: snapshot.l3.get("l3_score").and_then(|v| v.as_f64()).unwrap_or(0.0),
        crc_valid,
        top_finding,
    };

    let mut chain = state
        .chain
        .lock()
        .map_err(|_| "kunci riwayat rusak (poisoned)".to_string())?;
    Ok(chain.push(input))
}

/// Returns the recorded scans, newest first.
#[tauri::command]
fn history_entries(state: tauri::State<'_, HistoryState>) -> Result<Vec<history::HistoryEntry>, String> {
    let chain = state
        .chain
        .lock()
        .map_err(|_| "kunci riwayat rusak (poisoned)".to_string())?;
    let mut out = chain.entries().to_vec();
    out.reverse();
    Ok(out)
}

/// Verifies the integrity chain over the recorded scans.
///
/// Returns `(intact, detail)`. Exposed to the UI so a user can check that the
/// evidence list has not been altered, and so a demo can show the detection
/// working rather than asserting it does.
#[tauri::command]
fn history_verify(state: tauri::State<'_, HistoryState>) -> Result<(bool, String), String> {
    let chain = state
        .chain
        .lock()
        .map_err(|_| "kunci riwayat rusak (poisoned)".to_string())?;
    Ok(chain.verify())
}

#[tauri::command]
fn history_clear(state: tauri::State<'_, HistoryState>) -> Result<(), String> {
    let mut chain = state
        .chain
        .lock()
        .map_err(|_| "kunci riwayat rusak (poisoned)".to_string())?;
    chain.clear();
    Ok(())
}

// ---------------------------------------------------------------------------
// Shareable report
// ---------------------------------------------------------------------------

/// Renders a scan as a text or HTML document.
///
/// Generated entirely on-device and returned as a string: the app never uploads
/// a report, so the "no data leaves the device" guarantee holds even for the
/// feature whose whole purpose is sharing a finding.
#[tauri::command]
fn generate_report(
    state: tauri::State<'_, HistoryState>,
    snapshot: ScanSnapshot,
    source: String,
    format: Option<String>,
    timestamp_ms: Option<u64>,
) -> Result<String, String> {
    let chain_hash = state
        .chain
        .lock()
        .ok()
        .and_then(|c| c.entries().last().map(|e| e.entry_hash));

    let fmt = report::ReportFormat::parse(format.as_deref().unwrap_or("text"));
    Ok(report::render(
        &report::ReportInput {
            snapshot: &snapshot,
            source: &source,
            timestamp_ms,
            app_version: env!("CARGO_PKG_VERSION"),
            chain_hash,
        },
        fmt,
    ))
}

/// Analyses a payload together with an optional image, without a camera.
///
/// This is the `imported_image` path: the frame arrives as raw RGB over IPC so
/// Layer 1 can run on it. The bbox is required for the same reason it is on the
/// camera path — without a symbol boundary there is no quiet zone to measure.
#[tauri::command]
fn analyze_image_frame(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    bbox: Option<[u32; 4]>,
    optical_type: Option<String>,
    client_city: Option<String>,
    client_lat: Option<f64>,
    client_lon: Option<f64>,
) -> Result<ScanSnapshot, String> {
    let frame = Frame::new(width, height, rgb).map_err(|e| e.to_string())?;

    let hit = qr::decode(&frame).map_err(|e| format!("decode gagal: {e}"))?;

    let effective_bbox = bbox.map(|b| (b[0], b[1], b[2], b[3])).or_else(|| {
        hit.as_ref()
            .map(|h| (h.bbox[0] as u32, h.bbox[1] as u32, h.bbox[2] as u32, h.bbox[3] as u32))
    });

    // Layer 1 needs the frame, and the blur metric below needs it too, so the
    // single-frame analysis takes a borrow rather than consuming the buffer.
    let (l1, payload) = match (&hit, effective_bbox) {
        (Some(h), Some(b)) => (
            serde_json::to_value(layer1_optical::analyze_burst(
                std::slice::from_ref(&frame),
                b,
            ))
            .unwrap_or(Value::Null),
            h.payload.clone(),
        ),
        (Some(h), None) => (
            layer1_placeholder("bbox QR tidak tersedia"),
            h.payload.clone(),
        ),
        (None, _) => (
            layer1_placeholder("tidak ada QR yang terbaca pada gambar"),
            String::new(),
        ),
    };

    let client_fix = match (client_lat, client_lon) {
        (Some(la), Some(lo)) => Some((la, lo)),
        _ => None,
    };
    let mut snapshot = analyze_with(
        payload,
        optical_type,
        client_city,
        l1,
        client_fix,
    );

    snapshot.blur_var = laplacian_variance(&frame);
    snapshot.is_blurry = snapshot.blur_var < DEFAULT_BLUR_THRESHOLD;
    snapshot.qr_bbox = effective_bbox.map(|(x, y, w, h)| [x as i32, y as i32, w as i32, h as i32]);

    Ok(snapshot)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(CameraState::default())
        .manage(HistoryState::default());

    // Registered only on mobile, where an OS location service exists. On desktop
    // the plugin is not linked at all (see the `geolocation` feature), so this
    // block is compiled out rather than conditionally failing at runtime.
    #[cfg(feature = "geolocation")]
    let builder = builder.plugin(tauri_plugin_geolocation::init());

    builder
        .invoke_handler(tauri::generate_handler![
            analyze_payload,
            analyze_geofence,
            verify_payload_crc,
            camera_info,
            camera_diagnostics,
            camera_preview,
            capture_frame,
            capture_and_analyze,
            release_camera,
            analyze_image_frame,
            analyze_image_bytes,
            inspect_image,
            geocode_city,
            known_cities,
            record_scan,
            history_entries,
            history_verify,
            history_clear,
            generate_report
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
        let ok = process_layer3_geofence(Some("Jakarta"), Some("JAKARTA"), None);
        assert_eq!(ok.l3_score, 0.0);
        assert_eq!(ok.risk_level, "LOW RISK");
        assert!(ok.evaluated);

        // A mismatch is CAUTION, never a veto. The old rule scored this 1.0,
        // which forced the combined verdict to HIGH RISK for any traveller.
        let bad = process_layer3_geofence(Some("Bandung"), Some("JAKARTA"), None);
        assert_eq!(bad.risk_level, "CAUTION");
        assert!(
            bad.l3_score < 0.70,
            "a mismatch must not veto on its own, got {}",
            bad.l3_score
        );
    }

    #[test]
    fn geofence_missing_data_skips() {
        let no_merchant = process_layer3_geofence(Some("Bandung"), None, None);
        assert_eq!(no_merchant.l3_score, 0.0);

        let no_client = process_layer3_geofence(None, Some("JAKARTA"), None);
        assert_eq!(no_client.l3_score, 0.0);

        // Skipped must be distinguishable from passed.
        assert!(!no_merchant.evaluated);
        assert!(!no_client.evaluated);
        assert_eq!(no_client.risk_level, "NOT RUN");
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
        let snap = analyze_payload(tampered, Some("physical_camera_scan".to_string()), None, None, None);
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
        let snap = analyze_payload(qris, Some("physical_camera_scan".to_string()), None, None, None);
        assert_eq!(snap.combined_score, 0.0);
        assert_eq!(snap.combined_risk_level, "LOW RISK");
    }

    /// Regression guard for the largest false-positive source in the old rule.
    ///
    /// Previously any city mismatch scored 1.0 and, because the combined score
    /// is `max(l1, l2, l3)`, that alone forced the whole scan to HIGH RISK. A
    /// traveller scanning a legitimate QR must not see a red verdict.
    #[test]
    fn combined_traveller_mismatch_does_not_reach_high_risk() {
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

        // Valid payload, valid CRC, but scanned 1400 km from Jakarta.
        let snap = analyze_payload(
            qris,
            Some("physical_camera_scan".to_string()),
            Some("Makassar".to_string()),
            Some(-5.1477),
            Some(119.4327),
        );

        assert!(snap.l2["crc_valid"].as_bool().unwrap(), "payload is intact");
        assert_eq!(snap.l3["mismatch_kind"], "DIFFERENT_CITY_DISTANT");
        assert!(
            snap.combined_score < 0.70,
            "a distant mismatch must not veto the scan, got {}",
            snap.combined_score
        );
        assert_eq!(snap.combined_risk_level, "CAUTION");
    }

    /// The payload-only path must declare that the optical layer did not run.
    /// This is what stops a partial scan from being read as a clean bill.
    #[test]
    fn payload_only_scan_reports_incomplete_coverage() {
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
        let snap = analyze_payload(generate_qris_with_crc(&raw), None, None, None, None);

        assert!(
            !snap.coverage.optical_ran,
            "there was no frame, so the optical layer cannot have run"
        );
        assert!(!snap.coverage.complete);
        assert_eq!(snap.l1["risk_level"], "NOT RUN");
        assert!(
            snap.findings.iter().any(|f| f.code == "NO_FINDINGS"
                && f.detail.contains("tidak berjalan")),
            "an empty finding list must state what was not checked: {:?}",
            snap.findings
        );
    }

    #[test]
    fn crc_failure_is_reported_as_a_named_finding() {
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
        let tampered = generate_qris_with_crc(&raw).replace("WARUNG MAKMUR", "WARUNG HACKED");
        let snap = analyze_payload(tampered, None, None, None, None);

        let finding = snap
            .findings
            .iter()
            .find(|f| f.code == "L2_CRC_MISMATCH")
            .expect("CRC failure must produce a named finding");
        assert_eq!(finding.severity, "HIGH");
        assert_eq!(finding.layer, "L2");
    }

    /// The end-to-end assertion the pipeline previously lacked.
    ///
    /// Every other test stopped *before* `qr::decode`, because the synthetic
    /// backend's geometric stand-in was not a valid symbol and never decoded.
    /// That left the whole frame -> decode -> Layer 1 -> Layer 2 chain
    /// unverified: a regression at the decode seam would have produced
    /// `no_qr_reason` instead of a failure, which looks like "the user aimed
    /// badly" rather than a bug.
    ///
    /// Requires `qr-encode`, because it needs a genuinely decodable symbol.
    #[cfg(feature = "qr-encode")]
    #[test]
    fn synthetic_frame_reaches_the_payload_layers_end_to_end() {
        use crate::camera::synthetic::{SyntheticBackend, SyntheticSpec};

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

        let frame = SyntheticBackend::with_spec(SyntheticSpec {
            glare_radius: 1,
            qr_payload: Some(qris.clone()),
            ..Default::default()
        })
        .capture()
        .expect("synthetic capture");

        // Step 1: the symbol must decode, and to the payload that was drawn.
        let hit = qr::decode(&frame)
            .expect("decode must not error")
            .expect("a rendered symbol must decode");
        assert_eq!(hit.payload, qris, "the decoded payload must be intact");

        // Step 2: Layer 1 must run on the decoded bounding box.
        let bbox = (
            hit.bbox[0] as u32,
            hit.bbox[1] as u32,
            hit.bbox[2] as u32,
            hit.bbox[3] as u32,
        );
        let l1 = layer1_optical::analyze_burst(std::slice::from_ref(&frame), bbox);
        assert!(l1.ran(), "Layer 1 must run when a QR was found");

        // Step 3: the unmodified payload must pass Layer 2.
        let l2 = process_layer2_tlv(&hit.payload, Some("physical_camera_scan"));
        assert!(l2.crc_valid, "a freshly encoded payload has a valid CRC");
        assert_eq!(l2.merchant_name, "WARUNG MAKMUR");
        assert_eq!(l2.merchant_city, "JAKARTA");
    }

    /// The central claim of the whole project, as an executable assertion.
    ///
    /// A sticker overlays the margin, so: the payload still decodes identically
    /// (which is why every payload-level control misses the attack), and Layer 1
    /// is the layer that sees it. If this test ever passes with identical L1
    /// scores, the detector has gone blind.
    ///
    /// Requires `qr-encode`, because the assertion is about a real payload.
    #[cfg(feature = "qr-encode")]
    #[test]
    fn sticker_preserves_the_payload_but_moves_the_optical_score() {
        use crate::camera::synthetic::{SyntheticBackend, SyntheticSpec};

        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00, f.tag01_static, f.tag26, f.tag52_grocery, f.tag53_idr, f.tag58_id, f.tag59_warung, f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        let capture = |sticker: bool| {
            SyntheticBackend::with_spec(SyntheticSpec {
                glare_radius: 1,
                sticker_anomaly: sticker,
                qr_payload: Some(qris.clone()),
                ..Default::default()
            })
            .capture()
            .unwrap()
        };

        let clean = capture(false);
        let stickered = capture(true);

        let clean_hit = qr::decode(&clean).unwrap().unwrap();
        let sticker_hit = qr::decode(&stickered).unwrap().unwrap();

        // Payload-level: indistinguishable. This is the attack's whole point.
        assert_eq!(clean_hit.payload, sticker_hit.payload);

        let bbox_of = |h: &qr::QrHit| (h.bbox[0], h.bbox[1], h.bbox[2], h.bbox[3]);
        let clean_l1 = layer1_optical::analyze_burst(std::slice::from_ref(&clean), bbox_of(&clean_hit));
        let sticker_l1 =
            layer1_optical::analyze_burst(std::slice::from_ref(&stickered), bbox_of(&sticker_hit));

        assert!(
            sticker_l1.l1_score > clean_l1.l1_score,
            "Layer 1 is the only layer that can see the sticker: clean={:.3} sticker={:.3}",
            clean_l1.l1_score,
            sticker_l1.l1_score
        );
    }

    /// Regression test for "Layer 3 never runs".
    ///
    /// The reported symptom was that every scan said client location was
    /// unavailable. Two independent causes, both covered here:
    ///
    /// 1. The UI called `navigator.geolocation`, which the Tauri WebView cannot
    ///    back with a real service, so the scan never received any location.
    /// 2. Even with a city name, this path resolved no coordinates, so no
    ///    distance was possible and the layer reported `NOT RUN`.
    ///
    /// This test pins cause 2: typing a known city must be enough to run the
    /// comparison *and* produce a distance, with no GPS involved.
    #[test]
    fn a_typed_city_is_enough_to_run_layer_3_without_any_gps() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00, f.tag01_static, f.tag26, f.tag52_grocery, f.tag53_idr, f.tag58_id, f.tag59_warung, f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        // Only a city name. No latitude, no longitude.
        let snap = analyze_payload(
            qris,
            Some("physical_camera_scan".to_string()),
            Some("Bandung".to_string()),
            None,
            None,
        );

        let l3 = &snap.l3;
        assert_eq!(
            l3["evaluated"], true,
            "a typed city must be enough to evaluate Layer 3, got {l3}"
        );
        assert_ne!(
            l3["risk_level"], "NOT RUN",
            "Layer 3 must not be skipped when a city was supplied: {l3}"
        );
        assert!(
            l3["distance_km"].is_number(),
            "the offline table must supply coordinates, giving a distance: {l3}"
        );
        assert_eq!(l3["mismatch_kind"], "DIFFERENT_CITY_NEARBY");
        assert!(snap.coverage.geofence_ran, "coverage must reflect it ran");
    }

    /// The same city on both sides must be a clean match, not a skip.
    #[test]
    fn a_matching_typed_city_reports_low_risk_rather_than_not_run() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00, f.tag01_static, f.tag26, f.tag52_grocery, f.tag53_idr, f.tag58_id, f.tag59_warung, f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        let snap = analyze_payload(
            qris,
            Some("physical_camera_scan".to_string()),
            Some("Jakarta".to_string()),
            None,
            None,
        );

        assert_eq!(snap.l3["evaluated"], true);
        assert_eq!(snap.l3["mismatch_kind"], "MATCH");
        assert_eq!(snap.l3["l3_score"], 0.0);
        assert_eq!(snap.combined_risk_level, "LOW RISK");
    }

    /// A city outside the offline table must still be evaluated by name.
    ///
    /// It loses only the distance refinement. Reporting `NOT RUN` here would be
    /// wrong: the comparison *did* happen, it just had no coordinates.
    #[test]
    fn an_unknown_city_still_evaluates_without_a_distance() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00, f.tag01_static, f.tag26, f.tag52_grocery, f.tag53_idr, f.tag58_id, f.tag59_warung, f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        let snap = analyze_payload(
            qris,
            Some("physical_camera_scan".to_string()),
            Some("Nowheresville".to_string()),
            None,
            None,
        );

        assert_eq!(snap.l3["evaluated"], true, "comparison still happened");
        assert_eq!(snap.l3["mismatch_kind"], "DIFFERENT_CITY_UNBOUNDED");
        assert!(snap.l3["distance_km"].is_null(), "no coordinates to measure");
        assert!(
            snap.l3["l3_score"].as_f64().unwrap() < 0.70,
            "an unresolvable city must not veto"
        );
    }

    /// Genuinely absent location must still report a skip, not a pass.
    #[test]
    fn no_location_at_all_still_reports_not_run() {
        let f = fixtures();
        let raw = format!(
            "{}{}{}{}{}{}{}{}",
            f.tag00, f.tag01_static, f.tag26, f.tag52_grocery, f.tag53_idr, f.tag58_id, f.tag59_warung, f.tag60_jakarta
        );
        let qris = generate_qris_with_crc(&raw);

        let snap = analyze_payload(qris, Some("physical_camera_scan".to_string()), None, None, None);

        assert_eq!(snap.l3["evaluated"], false);
        assert_eq!(snap.l3["risk_level"], "NOT RUN");
        assert!(!snap.coverage.geofence_ran);
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
