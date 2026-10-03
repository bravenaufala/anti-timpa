//! Camera diagnostics.
//!
//! Camera failures are the hardest class of bug to debug in this app: they
//! depend on hardware, kernel drivers, desktop session type, and which other
//! process happens to hold the device. A single "capture failed" message is
//! useless in that situation.
//!
//! So every stage that can fail logs what it attempted and what it got back.
//! These go to stderr, which means `npm run tauri:dev` shows them directly in
//! the terminal, and on Android they land in logcat under the `ANTITIMPA`
//! tag.

use std::sync::atomic::{AtomicBool, Ordering};

/// Debug-level logging is opt-in: set `ANTITIMPA_CAM_DEBUG=1`.
///
/// Per-frame tracing is far too noisy to leave on, but it is useful when
/// frames arrive in an unexpected format.
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static INIT: std::sync::Once = std::sync::Once::new();

fn debug_enabled() -> bool {
    INIT.call_once(|| {
        let on = std::env::var("ANTITIMPA_CAM_DEBUG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        DEBUG_ENABLED.store(on, Ordering::Relaxed);
    });
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Writes a line to stderr, tagged so it can be filtered.
///
/// On Android, stderr from the Rust side is forwarded to logcat by Tauri's
/// runtime, so the same line is readable via:
///
/// ```text
/// adb logcat -s ANTITIMPA
/// ```
fn emit(level: &str, msg: &str) {
    eprintln!("[ANTITIMPA][camera][{level}] {msg}");
}

pub fn cam_info(msg: &str) {
    emit("INFO", msg);
}

pub fn cam_warn(msg: &str) {
    emit("WARN", msg);
}

pub fn cam_error(msg: &str) {
    emit("ERROR", msg);
}

/// Noisy per-frame tracing; silent unless `ANTITIMPA_CAM_DEBUG=1`.
pub fn cam_debug(msg: &str) {
    if debug_enabled() {
        emit("DEBUG", msg);
    }
}

/// Machine-readable snapshot of the camera subsystem for the UI.
///
/// Surfacing the camera state as a command lets the UI show the real state
/// directly rather than scraping logs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CameraDiagnostics {
    pub backend: String,
    pub ready: bool,
    pub synthetic: bool,
    /// Number of successful frame grabs since startup.
    pub captures_ok: u64,
    /// Number of failed frame grabs since startup.
    pub captures_failed: u64,
    /// Last error message, if any. Cleared on a successful capture.
    pub last_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_off_by_default() {
        // Guards against accidentally shipping per-frame tracing enabled,
        // which would flood stderr and slow capture measurably.
        if std::env::var("ANTITIMPA_CAM_DEBUG").is_err() {
            assert!(!debug_enabled());
        }
    }
}
