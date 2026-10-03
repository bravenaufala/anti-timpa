//! iOS native bridge (C-ABI).
//!
//! This is the receiving end of the camera pipeline on iOS: Swift hands RGBA
//! buffers here, and they land in a [`FrameSlot`] that
//! [`MobileCameraBackend`](super::MobileCameraBackend) drains.
//!
//! ## C ABI instead of JNI
//!
//! Android reaches Rust through the JVM, so its entry points are JNI functions
//! with mangled names (`Java_...`). iOS has no such runtime: Swift links the
//! Rust static library directly and calls ordinary `extern "C"` symbols. So
//! this module exports plain C names (`antitimpa_ios_*`) that the Swift side
//! (see `ios/CameraBridge.swift`) declares as `@_silgen_name` free functions.
//!
//! ## Keeping the symbols alive
//!
//! On Android a release build with LTO + `strip = true` removes these entry
//! points (nothing in Rust references them) unless `build.rs` pins them with
//! `-Wl,--undefined`. On iOS the Swift side references the symbols, so the
//! final link keeps them, but that only holds if the Swift caller is part of
//! the build. Confirm with `nm` on the linked binary, the same way the
//! Android build script confirms its `.so`.
//!
//! ## What is verified and what is not
//!
//! The Rust side (validation, the shared RGBA intake, the slot) is unit-tested
//! on the host under the `ios-bridge` feature, exactly like the Android bridge.
//! The Swift glue and the on-device capture session have not been built or
//! run; that needs macOS + Xcode. Treat this as a compile-ready scaffold, not
//! a verified feature.

use super::{convert_and_store, FrameSlot};
use crate::camera::log::{cam_debug, cam_error, cam_info};

use std::sync::{Mutex, OnceLock};

/// The process-wide camera frame slot.
///
/// `OnceLock` rather than `lazy_static` so this needs no extra dependency, and
/// it matches the Android bridge: one camera session per process means one
/// shared slot is the correct model, not merely a convenience.
static SLOT: OnceLock<Mutex<FrameSlot>> = OnceLock::new();

/// Installs the frame slot. Call once during app setup, before any frame
/// arrives. Subsequent calls return the existing handle.
pub fn install_slot() -> FrameSlot {
    let mutex = SLOT.get_or_init(|| Mutex::new(FrameSlot::new()));
    match mutex.lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => {
            cam_error("install_slot: mutex poisoned; memulihkan slot kamera");
            poisoned.into_inner().clone()
        }
    }
}

/// Pushes one frame from the native camera into the slot.
///
/// C-ABI entry point for `CameraBridge.swift`. `buffer` must point to a
/// contiguous RGBA8888 buffer of at least `width * height * 4` bytes.
///
/// Returns `true` when accepted, `false` when rejected (null buffer, bad
/// dimensions, or buffer too small). A status rather than a trap keeps the
/// Swift capture callback free of error handling on a hot path.
///
/// # Safety
/// `buffer` must be either NULL or point to `len` readable bytes for the
/// duration of the call.
#[no_mangle]
pub unsafe extern "C" fn antitimpa_ios_push_frame(
    width: i32,
    height: i32,
    rotation_degrees: i32,
    buffer: *const u8,
    len: usize,
) -> bool {
    push_frame_impl(
        width.max(0) as u32,
        height.max(0) as u32,
        rotation_degrees,
        buffer,
        len,
    )
}

/// Actual frame intake, separated from the C signature so it can be unit tested
/// on a desktop host without an iOS toolchain.
///
/// # Safety
/// Same contract as the entry point above.
pub unsafe fn push_frame_impl(
    width: u32,
    height: u32,
    rotation_degrees: i32,
    buffer_ptr: *const u8,
    buffer_len: usize,
) -> bool {
    let Some(mutex) = SLOT.get() else {
        cam_error("push_frame: slot belum di-install (install_slot belum dipanggil)");
        return false;
    };

    let guard = match mutex.lock() {
        Ok(g) => g,
        Err(_) => {
            cam_error("push_frame: mutex poisoned; frame dibuang");
            return false;
        }
    };

    // Log the first few frames at INFO, then drop to DEBUG, matching Android so
    // the log tag reads the same on both platforms.
    let count = guard.pushed_count();
    if count < 3 {
        cam_info(&format!(
            "frame #{count} diterima dari native: {width}x{height} rotasi={rotation_degrees} \
             ({} byte)",
            width as usize * height as usize * 4
        ));
    } else {
        cam_debug(&format!("push_frame: {width}x{height} rotasi={rotation_degrees}"));
    }

    // Identical validation and alpha strip to the Android path: one shared
    // `convert_and_store`, so the two platforms cannot diverge.
    match convert_and_store(&guard, width, height, rotation_degrees, buffer_ptr, buffer_len) {
        Ok(()) => true,
        Err(reason) => {
            cam_error(&format!("push_frame: {reason}"));
            false
        }
    }
}

/// Marks the native stream active/inactive.
///
/// Swift calls this when the `AVCaptureSession` starts and stops, so `capture()`
/// can tell "session not started" from "running but no frame yet".
#[no_mangle]
pub extern "C" fn antitimpa_ios_set_stream_active(active: bool) {
    let Some(mutex) = SLOT.get() else {
        cam_error("set_stream_active: slot belum di-install");
        return;
    };
    match mutex.lock() {
        Ok(slot) => {
            slot.set_stream_active(active);
            cam_info(&format!(
                "stream aktif={active} (total frame diterima={})",
                slot.pushed_count()
            ));
        }
        Err(_) => cam_error("set_stream_active: mutex poisoned"),
    }
}

/// Reports how many frames the native side has delivered.
#[no_mangle]
pub extern "C" fn antitimpa_ios_frames_received() -> u64 {
    SLOT.get()
        .and_then(|m| m.lock().ok().map(|s| s.pushed_count()))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::mobile::Rotation;

    /// Serializes these tests: they share the process-wide `SLOT`.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn push(width: u32, height: u32, rotation: i32) -> bool {
        let buf = vec![255u8; (width * height * 4) as usize];
        unsafe { push_frame_impl(width, height, rotation, buf.as_ptr(), buf.len()) }
    }

    #[test]
    fn entry_point_symbols_exist_with_the_expected_signatures() {
        // Taking the address proves the symbol is defined with the exact
        // identifier Swift will look up. A typo here only surfaces at link time
        // on device, mirroring the Android `UnsatisfiedLinkError` class of bug.
        let _: unsafe extern "C" fn(i32, i32, i32, *const u8, usize) -> bool =
            antitimpa_ios_push_frame;
        let _: extern "C" fn(bool) = antitimpa_ios_set_stream_active;
        let _: extern "C" fn() -> u64 = antitimpa_ios_frames_received;
    }

    #[test]
    fn push_accepts_a_valid_buffer_and_strips_alpha() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        slot.clear();

        assert!(push(4, 4, 0));
        let frame = slot.take().expect("frame should be stored");
        assert_eq!((frame.width, frame.height), (4, 4));
        assert_eq!(frame.rgb.len(), 4 * 4 * 3, "alpha stripped at intake");
    }

    #[test]
    fn push_rejects_null_pointer() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        let accepted = unsafe { push_frame_impl(4, 4, 0, std::ptr::null(), 64) };
        assert!(!accepted);
    }

    #[test]
    fn push_rejects_undersized_buffer() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        let small = vec![0u8; 10];
        let accepted = unsafe { push_frame_impl(4, 4, 0, small.as_ptr(), small.len()) };
        assert!(!accepted, "a short read would be silent corruption");
    }

    #[test]
    fn push_rejects_zero_dimensions() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        assert!(!push(0, 4, 0));
        assert!(!push(4, 0, 0));
    }

    #[test]
    fn rotation_hint_is_recorded() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        slot.clear();
        assert!(push(2, 2, 90));
        assert_eq!(slot.take().unwrap().rotation, Rotation::Deg90);
    }

    #[test]
    fn stream_active_flag_round_trips() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        antitimpa_ios_set_stream_active(true);
        assert!(slot.is_stream_active());
        antitimpa_ios_set_stream_active(false);
        assert!(!slot.is_stream_active());
    }

    #[test]
    fn frames_received_counter_advances() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        let before = antitimpa_ios_frames_received();
        assert!(push(2, 2, 0));
        assert_eq!(antitimpa_ios_frames_received(), before + 1);
    }

    #[test]
    fn copied_buffer_survives_original_drop() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        slot.clear();
        {
            let buf = vec![7u8; 2 * 2 * 4];
            unsafe {
                assert!(push_frame_impl(2, 2, 0, buf.as_ptr(), buf.len()));
            }
        }
        let frame = slot.take().expect("frame must survive source drop");
        assert!(frame.rgb.iter().all(|&b| b == 7));
    }
}
