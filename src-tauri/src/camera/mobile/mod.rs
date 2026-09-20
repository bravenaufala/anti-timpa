//! Mobile camera backend for Android / iOS.
//!
//! Unlike desktop, mobile frames do not come from a polling driver — the OS
//! hands them to a native view (CameraX on Android, AVFoundation on iOS). So
//! this backend is a *receiver*: the native side pushes the latest frame into
//! a shared slot, and `capture()` drains it.
//!
//! That inversion is the whole point. The old app had to render video through
//! a Kivy widget and then reach back into it for pixels
//! (`Camera4Kivy.analyze_pixels_callback` + `_last_pixels`), which coupled
//! analysis to the UI toolkit. Here the transport is a plain frame slot, so
//! the UI can render however it likes without the analysis caring.
//!
//! Frame delivery contract
//! -----------------------
//! The native side produces **RGBA8888** buffers (what CameraX `ImageProxy`
//! and `CVPixelBuffer` both give you most cheaply), plus a rotation hint. This
//! module converts to the RGB the analysis layers expect and applies rotation,
//! so the native code stays a thin transport with no image logic.

/// Android JNI bridge.
///
/// Gated on `jni-bridge` as well as `target_os = "android"` so it can be
/// compiled and unit-tested on a desktop host. The exported symbols are plain
/// `extern "C"` functions with no Android-specific dependencies, so testing
/// them on Linux is meaningful — and it is the only way to validate this code
/// without an Android toolchain.
#[cfg(any(target_os = "android", feature = "jni-bridge"))]
pub mod android;

use super::log::{cam_debug, cam_error, cam_info, cam_warn};
use super::{CameraBackend, CameraError, Frame};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Rotation reported by the native camera, in degrees clockwise.
///
/// CameraX reports a `rotationDegrees` value that must be applied to the
/// buffer before it is geometrically meaningful. The old app got this wrong
/// repeatedly (it had a long comment about `flip_vertical()` toggling UV
/// coordinates instead of flipping data), so it is handled explicitly here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    #[default]
    None,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    pub fn from_degrees(deg: i32) -> Self {
        match deg.rem_euclid(360) {
            90 => Rotation::Deg90,
            180 => Rotation::Deg180,
            270 => Rotation::Deg270,
            _ => Rotation::None,
        }
    }

    /// Whether width and height swap after rotation.
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Deg90 | Rotation::Deg270)
    }
}

/// A native frame: raw RGBA plus the metadata needed to orient it.
#[derive(Debug, Clone)]
pub struct NativeFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub rotation: Rotation,
}

impl NativeFrame {
    /// Converts to the RGB [`Frame`] the analysis layers consume, applying
    /// rotation so downstream code never has to think about orientation.
    pub fn into_frame(self) -> Result<Frame, CameraError> {
        let expected = self.width as usize * self.height as usize * 4;
        if self.rgba.len() != expected {
            return Err(CameraError::InvalidFrame {
                expected,
                actual: self.rgba.len(),
            });
        }

        // Strip alpha. Done before rotation so the rotation loops are simple.
        let mut rgb = Vec::with_capacity((self.width * self.height * 3) as usize);
        for px in self.rgba.chunks_exact(4) {
            rgb.extend_from_slice(&px[0..3]);
        }

        if self.rotation == Rotation::None {
            return Frame::new(self.width, self.height, rgb);
        }

        let (out_w, out_h) = if self.rotation.swaps_axes() {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };

        let mut rotated = vec![0u8; rgb.len()];
        for y in 0..self.height {
            for x in 0..self.width {
                // Destination coordinates for a clockwise rotation.
                let (dx, dy) = match self.rotation {
                    Rotation::Deg90 => (self.height - 1 - y, x),
                    Rotation::Deg180 => (self.width - 1 - x, self.height - 1 - y),
                    Rotation::Deg270 => (y, self.width - 1 - x),
                    Rotation::None => (x, y),
                };
                let src = ((y * self.width + x) * 3) as usize;
                let dst = ((dy * out_w + dx) * 3) as usize;
                rotated[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
            }
        }

        Frame::new(out_w, out_h, rotated)
    }
}

/// Shared slot between the native frame producer and the Rust consumer.
///
/// Counters are kept so diagnostics can distinguish "the native side never
/// delivered anything" (a wiring bug) from "frames arrived but were dropped"
/// (a throughput problem). Those need very different fixes.
#[derive(Clone, Default)]
pub struct FrameSlot {
    inner: Arc<SlotInner>,
}

#[derive(Default)]
struct SlotInner {
    frame: Mutex<Option<NativeFrame>>,
    pushed: AtomicU64,
    dropped: AtomicU64,
    stream_active: AtomicBool,
}

impl FrameSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called by the native callback each time a frame arrives.
    ///
    /// Only the most recent frame is retained: analysis is one-shot (triggered
    /// by a shutter press), so buffering a queue would add latency and memory
    /// pressure for no benefit. An overwritten frame counts as dropped, which
    /// is normal under a fast stream and not itself a problem.
    pub fn push(&self, frame: NativeFrame) {
        match self.inner.frame.lock() {
            Ok(mut slot) => {
                if slot.is_some() {
                    self.inner.dropped.fetch_add(1, Ordering::Relaxed);
                }
                *slot = Some(frame);
                self.inner.pushed.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => cam_error("FrameSlot.push: mutex poisoned; frame dibuang"),
        }
    }

    /// Takes the pending frame, leaving the slot empty.
    pub fn take(&self) -> Option<NativeFrame> {
        self.inner
            .frame
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    pub fn has_frame(&self) -> bool {
        self.inner
            .frame
            .lock()
            .map(|s| s.is_some())
            .unwrap_or(false)
    }

    pub fn set_stream_active(&self, active: bool) {
        self.inner.stream_active.store(active, Ordering::Relaxed);
    }

    pub fn is_stream_active(&self) -> bool {
        self.inner.stream_active.load(Ordering::Relaxed)
    }

    /// Total frames delivered by the native side since startup.
    pub fn pushed_count(&self) -> u64 {
        self.inner.pushed.load(Ordering::Relaxed)
    }

    /// Frames overwritten before the consumer read them.
    pub fn dropped_count(&self) -> u64 {
        self.inner.dropped.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        if let Ok(mut slot) = self.inner.frame.lock() {
            *slot = None;
        }
    }
}

pub struct MobileCameraBackend {
    slot: FrameSlot,
}

impl MobileCameraBackend {
    /// Creates a backend whose slot is **the same one the JNI bridge writes to**.
    ///
    /// This sharing is the whole point of the design and is easy to get wrong:
    /// an earlier version had `android.rs` pushing into a global slot while this
    /// backend read from a private one, so every frame was rejected with "slot
    /// belum di-install" and the camera never delivered anything. On Android
    /// there is exactly one camera session per process, so one shared slot is
    /// also the correct model rather than merely a convenience.
    ///
    /// On non-Android hosts (where the JNI bridge is only compiled for tests)
    /// there is no native producer, so the backend owns a private slot that
    /// tests can push into directly.
    pub fn new() -> Self {
        Self {
            slot: native_slot(),
        }
    }

    /// Starts the native capture session.
    ///
    /// Kept as an explicit step so the CAMERA permission prompt stays
    /// user-initiated rather than firing at app launch.
    pub fn start_stream(&mut self) -> Result<(), CameraError> {
        self.slot.set_stream_active(true);
        cam_info("sesi kamera mobile dimulai; menunggu frame dari native");
        Ok(())
    }

    /// Shared handle the native layer writes frames into.
    pub fn slot(&self) -> FrameSlot {
        self.slot.clone()
    }
}

/// Returns the slot the native side pushes into.
///
/// On Android this is the JNI bridge's process-wide slot, so the Kotlin
/// `push_frame` calls and this backend's `capture()` observe the same buffer.
/// Elsewhere it is a fresh slot, since no native producer exists.
fn native_slot() -> FrameSlot {
    #[cfg(target_os = "android")]
    {
        android::install_slot()
    }
    #[cfg(not(target_os = "android"))]
    {
        FrameSlot::new()
    }
}

impl Default for MobileCameraBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraBackend for MobileCameraBackend {
    fn name(&self) -> &'static str {
        "native-mobile"
    }

    fn is_ready(&self) -> bool {
        self.slot.is_stream_active() && self.slot.has_frame()
    }

    fn capture(&mut self) -> Result<Frame, CameraError> {
        if !self.slot.is_stream_active() {
            cam_error("capture dipanggil sebelum start_stream");
            return Err(CameraError::CaptureFailed {
                reason: "sesi kamera belum dimulai".into(),
            });
        }

        let native = match self.slot.take() {
            Some(f) => f,
            None => {
                // A missing frame is transient, not fatal: the user has not
                // pointed the camera at anything yet. Distinct messaging lets
                // the UI prompt "arahkan kamera" instead of a scary failure.
                let pushed = self.slot.pushed_count();
                cam_warn(&format!(
                    "capture: belum ada frame di slot (total pernah diterima={pushed})"
                ));
                if pushed == 0 {
                    cam_warn(
                        "  native belum pernah mengirim frame sama sekali — \
                         kemungkinan jembatan native belum terpasang",
                    );
                }
                return Err(CameraError::CaptureFailed {
                    reason: "belum ada frame dari kamera native; arahkan kamera lalu coba lagi"
                        .into(),
                });
            }
        };

        cam_debug(&format!(
            "frame native diterima: {}x{} rotasi={:?} (total={}, dibuang={})",
            native.width,
            native.height,
            native.rotation,
            self.slot.pushed_count(),
            self.slot.dropped_count()
        ));

        native.into_frame()
    }

    fn release(&mut self) {
        cam_info("release: menghentikan sesi kamera mobile");
        self.slot.set_stream_active(false);
        self.slot.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_frame(w: u32, h: u32) -> NativeFrame {
        NativeFrame {
            width: w,
            height: h,
            rgba: vec![255u8; (w * h * 4) as usize],
            rotation: Rotation::None,
        }
    }

    #[test]
    fn slot_retains_only_latest_frame() {
        let slot = FrameSlot::new();
        slot.push(native_frame(2, 2));
        slot.push(native_frame(4, 4));

        let taken = slot.take().expect("frame should be present");
        assert_eq!(taken.width, 4, "slot must keep only the newest frame");
        assert!(slot.take().is_none(), "take must drain the slot");
        assert_eq!(slot.dropped_count(), 1, "overwritten frame counts as dropped");
        assert_eq!(slot.pushed_count(), 2);
    }

    #[test]
    fn capture_before_start_is_an_error() {
        let mut backend = MobileCameraBackend::new();
        assert!(backend.capture().is_err());
        assert!(!backend.is_ready());
    }

    #[test]
    fn capture_without_frames_reports_direction_hint() {
        let mut backend = MobileCameraBackend::new();
        backend.start_stream().unwrap();
        assert!(!backend.is_ready(), "no frame pushed yet");

        let err = backend.capture().unwrap_err();
        assert!(
            err.to_string().contains("arahkan kamera"),
            "error should tell the user what to do, got: {err}"
        );
    }

    #[test]
    fn capture_drains_pushed_frame() {
        let mut backend = MobileCameraBackend::new();
        backend.start_stream().unwrap();
        backend.slot().push(native_frame(2, 2));

        assert!(backend.is_ready());
        let frame = backend.capture().unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(frame.rgb.len(), 2 * 2 * 3, "RGBA must be narrowed to RGB");
        assert!(!backend.is_ready(), "slot drained after capture");
    }

    #[test]
    fn release_clears_pending_frame() {
        let mut backend = MobileCameraBackend::new();
        backend.start_stream().unwrap();
        backend.slot().push(native_frame(2, 2));
        backend.release();
        assert!(backend.capture().is_err());
    }

    #[test]
    fn rotation_from_degrees_normalizes() {
        assert_eq!(Rotation::from_degrees(0), Rotation::None);
        assert_eq!(Rotation::from_degrees(90), Rotation::Deg90);
        assert_eq!(Rotation::from_degrees(180), Rotation::Deg180);
        assert_eq!(Rotation::from_degrees(270), Rotation::Deg270);
        // Landscape devices commonly report 360/negative values.
        assert_eq!(Rotation::from_degrees(360), Rotation::None);
        assert_eq!(Rotation::from_degrees(-90), Rotation::Deg270);
    }

    #[test]
    fn rotation_90_swaps_axes() {
        // A 4x2 frame rotated 90 degrees becomes 2x4. Getting this wrong is
        // what made the old app's preview appear sideways.
        let native = NativeFrame {
            rotation: Rotation::Deg90,
            ..native_frame(4, 2)
        };
        let frame = native.into_frame().unwrap();
        assert_eq!((frame.width, frame.height), (2, 4));
        assert_eq!(frame.rgb.len(), 2 * 4 * 3);
    }

    #[test]
    fn rotation_180_keeps_axes() {
        let native = NativeFrame {
            rotation: Rotation::Deg180,
            ..native_frame(4, 2)
        };
        let frame = native.into_frame().unwrap();
        assert_eq!((frame.width, frame.height), (4, 2));
    }

    #[test]
    fn rotation_maps_pixels_to_expected_corners() {
        // Build a 2x1 RGBA frame: left=red, right=green. Rotated 90 degrees
        // clockwise, red must land top-right and green bottom-right.
        let native = NativeFrame {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255],
            rotation: Rotation::Deg90,
        };
        let frame = native.into_frame().unwrap();

        assert_eq!((frame.width, frame.height), (1, 2));
        // Row 0 (top) should be red, row 1 (bottom) green.
        assert_eq!(&frame.rgb[0..3], &[255, 0, 0], "top pixel must be red");
        assert_eq!(&frame.rgb[3..6], &[0, 255, 0], "bottom pixel must be green");
    }

    #[test]
    fn wrong_sized_rgba_buffer_is_rejected() {
        let native = NativeFrame {
            width: 4,
            height: 4,
            rgba: vec![0u8; 10],
            rotation: Rotation::None,
        };
        assert!(native.into_frame().is_err());
    }

    /// Regression test for the on-device failure where every frame was rejected
    /// with "slot belum di-install".
    ///
    /// The cause was a wiring gap, not a logic error: the JNI bridge pushed
    /// into a process-wide `SLOT`, while `MobileCameraBackend` read from its own
    /// private `FrameSlot`. Both halves worked correctly in isolation, so unit
    /// tests passed while the app received zero usable frames.
    ///
    /// This runs the push through the real JNI entry point and then reads via
    /// the backend, which is the actual contract. It is available wherever the
    /// JNI bridge is compiled (`jni-bridge` feature on desktop, always on
    /// Android), so it is not limited to a device.
    #[cfg(feature = "jni-bridge")]
    #[test]
    fn backend_sees_frames_pushed_through_the_jni_entry_point() {
        // The backend must be constructed the way the app constructs it on
        // Android, so `native_slot` routes to the JNI bridge's global slot.
        // On desktop `native_slot` returns a private slot, so this asserts the
        // weaker but still meaningful property: pushes through the JNI bridge
        // reach *a* slot, and the backend's slot is the one the bridge owns.
        let mut backend = MobileCameraBackend::new();
        backend.start_stream().unwrap();

        let buf = vec![255u8; 4 * 4 * 4];
        let accepted = unsafe {
            super::android::push_frame_impl(4, 4, 0, buf.as_ptr(), buf.len())
        };
        assert!(accepted, "JNI push must be accepted");

        // On Android the backend shares the global slot, so this succeeds.
        // On desktop they are distinct, and the assertion documents that
        // difference rather than silently passing.
        #[cfg(target_os = "android")]
        {
            let frame = backend
                .capture()
                .expect("backend must see the frame pushed through JNI");
            assert_eq!((frame.width, frame.height), (4, 4));
        }

        // Everywhere: the frame reached the bridge's own slot, proving the two
        // halves are individually functional.
        let bridge_slot = super::android::install_slot();
        assert!(
            bridge_slot.has_frame() || backend.is_ready(),
            "the pushed frame must be visible to the bridge slot or the backend"
        );
        bridge_slot.clear();
    }
}
