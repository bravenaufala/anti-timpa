//! Camera abstraction for the Tauri rewrite.
//!
//! The prototype's goal is to prove that the same command surface works on
//! desktop and mobile even though the implementation differs per platform.
//! [`CameraBackend`] is the seam:
//!
//! * `nokhwa`       -> desktop webcams (Linux/macOS/Windows)
//! * `MobileCamera` -> Android/iOS, frames pushed from a native plugin
//! * `Synthetic`    -> deterministic fallback so the pipeline is always testable

pub mod log;
pub mod preview;
pub mod synthetic;

#[cfg(feature = "desktop-camera")]
pub mod desktop;

// Also compiled on desktop under `jni-bridge` / `ios-bridge`, so the mobile
// bridges' unit tests can run without a device toolchain.
#[cfg(any(
    target_os = "android",
    target_os = "ios",
    feature = "jni-bridge",
    feature = "ios-bridge"
))]
pub mod mobile;

use serde::{Deserialize, Serialize};

/// A decoded camera frame in the layout the analysis layers expect.
///
/// `rgb` is row-major, 8 bits per channel, 3 channels, matching what
/// `image::RgbImage` produces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB8 pixels. Length must equal `width * height * 3`.
    pub rgb: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, rgb: Vec<u8>) -> Result<Self, CameraError> {
        let expected = width as usize * height as usize * 3;
        if rgb.len() != expected {
            return Err(CameraError::InvalidFrame {
                expected,
                actual: rgb.len(),
            });
        }
        Ok(Self {
            width,
            height,
            rgb,
        })
    }

    /// Converts to a grayscale buffer using Rec. 601 luma weighting.
    pub fn to_gray(&self) -> Vec<u8> {
        let mut gray = Vec::with_capacity((self.width * self.height) as usize);
        for px in self.rgb.chunks_exact(3) {
            let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
            let luma = 0.299 * r + 0.587 * g + 0.114 * b;
            gray.push(luma.round().clamp(0.0, 255.0) as u8);
        }
        gray
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CameraError {
    Unsupported { platform: String },
    NotFound,
    PermissionDenied,
    CaptureFailed { reason: String },
    InvalidFrame { expected: usize, actual: usize },
}

impl std::fmt::Display for CameraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CameraError::Unsupported { platform } => {
                write!(f, "Kamera tidak didukung pada platform ini: {platform}")
            }
            CameraError::NotFound => write!(f, "Perangkat kamera tidak ditemukan"),
            CameraError::PermissionDenied => write!(f, "Izin kamera ditolak"),
            CameraError::CaptureFailed { reason } => {
                write!(f, "Gagal menangkap frame: {reason}")
            }
            CameraError::InvalidFrame { expected, actual } => write!(
                f,
                "Ukuran frame tidak valid: harap {expected} byte, dapat {actual}"
            ),
        }
    }
}

impl std::error::Error for CameraError {}

/// Common surface every platform backend implements.
pub trait CameraBackend: Send {
    /// Human-readable backend name, surfaced to the UI for diagnostics.
    fn name(&self) -> &'static str;

    /// Whether a usable camera was actually opened.
    fn is_ready(&self) -> bool;

    /// Grabs one frame. One-shot by design: continuous per-frame decode can
    /// cause hangs, so the UI pulls on demand.
    fn capture(&mut self) -> Result<Frame, CameraError>;

    /// Releases the device. Called on app shutdown.
    ///
    /// Implementations must tolerate this being called spuriously and must
    /// remain re-openable afterwards. React StrictMode unmounts components
    /// once in development, firing the cleanup effect on a throwaway mount.
    fn release(&mut self);
}

/// Returns the backend appropriate for the current platform.
///
/// The function is infallible: on a platform where capture is unavailable the
/// caller gets a `Synthetic` backend, keeping the pipeline exercisable. This
/// provides the "mode simulasi" fallback so the UI never has to special-case
/// a missing camera.
pub fn default_backend() -> Box<dyn CameraBackend> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        log::cam_info("platform mobile terdeteksi; memakai penerima frame native");
        Box::new(mobile::MobileCameraBackend::new())
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        #[cfg(feature = "desktop-camera")]
        {
            // Override for unusual setups (e.g. several cameras).
            // `ANTITIMPA_CAMERA_INDEX=2` pins a specific device.
            if let Ok(raw) = std::env::var("ANTITIMPA_CAMERA_INDEX") {
                match raw.parse::<u32>() {
                    Ok(index) => {
                        log::cam_info(&format!("index kamera dipaksa lewat env: {index}"));
                        match desktop::DesktopCameraBackend::open(index) {
                            Ok(backend) => return Box::new(backend),
                            Err(e) => log::cam_warn(&format!(
                                "kamera index={index} dari env gagal dibuka ({e}); \
                                 melanjutkan ke pemindaian otomatis"
                            )),
                        }
                    }
                    Err(_) => log::cam_warn(&format!(
                        "ANTITIMPA_CAMERA_INDEX='{raw}' bukan angka; diabaikan"
                    )),
                }
            }

            let candidates = desktop::list_device_indices();
            if candidates.is_empty() {
                log::cam_warn(
                    "tidak ada kamera terdeteksi; memakai backend sintetik",
                );
                return Box::new(synthetic::SyntheticBackend::default());
            }

            log::cam_info(&format!("memindai kamera pada index: {candidates:?}"));

            // Try each device until one opens and yields a frame. Opening
            // successfully is not sufficient: UVC metadata nodes open happily
            // but never deliver video, so the warm-up check inside `open` is
            // what distinguishes a usable capture device.
            for index in candidates {
                match desktop::DesktopCameraBackend::open(index) {
                    Ok(backend) => {
                        log::cam_info(&format!("memakai kamera index={index}"));
                        return Box::new(backend);
                    }
                    Err(e) => log::cam_warn(&format!(
                        "kamera index={index} gagal ({e}); mencoba device berikutnya"
                    )),
                }
            }

            log::cam_warn(
                "semua kamera terdeteksi gagal dibuka; memakai backend sintetik. \
                 Cek: dipakai proses lain? izin /dev/videoN?",
            );
            return Box::new(synthetic::SyntheticBackend::default());
        }
        #[cfg(not(feature = "desktop-camera"))]
        {
            log::cam_warn(
                "fitur `desktop-camera` tidak aktif; memakai backend sintetik. \
                 Jalankan dengan --features desktop-camera untuk kamera asli.",
            );
            Box::new(synthetic::SyntheticBackend::default())
        }
    }
}
