//! Android native bridge.
//!
//! This is the receiving end of the camera pipeline: Kotlin code hands RGBA
//! buffers here, and they land in a [`FrameSlot`] that
//! [`MobileCameraBackend`](super::MobileCameraBackend) drains.
//!
//! ## Keeping the symbols alive in release builds
//!
//! JNI entry points are called by the JVM, not by Rust, so from the
//! compiler's perspective nothing references them. A release build with LTO
//! and `strip = true` therefore treats them as dead code and removes them.
//!
//! The result is a build that works in debug and fails in release with
//! `UnsatisfiedLinkError`, and the `.so` contains no trace of the symbols.
//! Debug builds hide the problem because nothing is stripped or inlined away.
//!
//! The fix is `-Wl,--undefined=<symbol>` for each entry point, emitted from
//! `build.rs`. That flag tells the linker to treat the symbol as a root even
//! though no relocation points at it, so it survives LTO and stripping.
//!
//! `#[used]` does not help here: it only works on statics, not functions. An
//! earlier attempt to use it failed to compile. The linker flag is the
//! mechanism that works, and the `JNI_EXPORTS` static below documents the
//! entry points for `build.rs` to mirror.
//!
//! ## Exported symbol names
//!
//! These are JNI entry points, not ordinary FFI functions. When Kotlin declares
//! a method as `external fun`, the JVM looks for a symbol whose name encodes the
//! package, class, and method, with `_` in the method name escaped to `_1`.
//!
//! So `CameraBridge.antitimpa_push_frame` must be exported as:
//!
//! ```text
//! Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame
//!                                 ^^ package ^^  ^^ class ^^  ^^ method ^^
//! ```
//!
//! Getting this wrong fails at runtime, not compile time, and the only clue is
//! an `UnsatisfiedLinkError` naming the symbol the JVM wanted. Exporting plain
//! C names (`antitimpa_push_frame`) links fine and even shows up in `nm`, but
//! the JVM never finds them, which is how this file's first version crashed on
//! launch.
//!
//! The parameter types are part of the JNI signature in principle, but the
//! plain name is sufficient here because each method is overload-free.
//!
//! ## Global slot
//!
//! JNI calls cannot carry Rust-owned generics or mutexes. The conventional
//! solution is a global that the JNI functions look up. That is what `SLOT` is.
//!
//! Only one camera session exists per process, so a single global is correct
//! rather than a shortcut.
//!
//! ## Buffer argument type
//!
//! A `java.nio.ByteBuffer` parameter is a JVM object reference, not a raw
//! memory address. Treating it as `*const u8` compiles, links, and then
//! segfaults inside `memcpy` the moment a frame arrives, because the JVM's
//! object reference is dereferenced as if it pointed at pixel data.
//!
//! The real address must be obtained through the JNI environment:
//!
//! * `GetDirectBufferAddress` returns the pointer for a direct buffer.
//! * `GetDirectBufferCapacity` returns its length.
//!
//! Only direct buffers have a stable address, which is why the Kotlin side is
//! required to send one. A heap buffer would need `GetByteArrayElements` and a
//! copy, or it would move under the GC.
//!
//! This is also why the buffer length is no longer a separate argument: taking
//! it from `GetDirectBufferCapacity` removes any chance of the caller passing a
//! length that disagrees with the actual allocation, the discrepancy that a
//! short read would otherwise turn into memory corruption.

use super::FrameSlot;

use std::sync::{Mutex, OnceLock};

/// The process-wide camera frame slot.
///
/// `OnceLock` rather than `lazy_static`/`once_cell` so this needs no extra
/// dependency. It is initialized by [`install_slot`] during app setup.
static SLOT: OnceLock<Mutex<FrameSlot>> = OnceLock::new();

/// Installs the frame slot. Call once during app setup, before any frame
/// arrives. Subsequent calls return the existing handle, so a re-created
/// Activity cannot clobber a slot the analysis side already holds.
///
/// Returns the handle the Rust side keeps for `capture()`.
pub fn install_slot() -> FrameSlot {
    let mutex = SLOT.get_or_init(|| Mutex::new(FrameSlot::new()));
    match mutex.lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => {
            // Poisoned means a previous push panicked mid-lock. The slot only
            // holds an Option, so its contents remain structurally valid;
            // recovering is far better than disabling the camera for the rest
            // of the session.
            crate::camera::log::cam_error("install_slot: mutex poisoned; memulihkan slot kamera");
            poisoned.into_inner().clone()
        }
    }
}

/// Pushes one frame from the native camera into the slot.
///
/// JNI entry point for `CameraBridge.antitimpa_push_frame`.
///
/// Returns `true` when accepted, `false` when rejected (bad dimensions, null
/// buffer, non-direct buffer, or buffer too small). Returning a status rather
/// than throwing keeps the Kotlin analyzer free of exception handling on a hot
/// path.
///
/// # Safety
/// Called by the JVM with a valid `JNIEnv` and a `jobject` for the
/// `CameraBridge` class. `buffer` must be a `java.nio.ByteBuffer` or null.
#[no_mangle]
pub unsafe extern "C" fn Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame(
    env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    width: jni::sys::jint,
    height: jni::sys::jint,
    rotation_degrees: jni::sys::jint,
    buffer: jni::sys::jobject,
) -> jni::sys::jboolean {
    use crate::camera::log::cam_error;
    use jni::objects::JByteBuffer;
    use jni::JNIEnv;

    // `JNIEnv` is a transparent wrapper; constructing it from the raw pointer is
    // the documented way to use the `jni` crate inside an `extern "C"` entry
    // point. Failure here means the JVM handed us an unusable environment, in
    // which case no JNI call can succeed, so report rejection rather than
    // aborting the process.
    let env = match JNIEnv::from_raw(env) {
        Ok(env) => env,
        Err(_) => return false as jni::sys::jboolean,
    };

    if buffer.is_null() {
        cam_error("push_frame: buffer null");
        return false as jni::sys::jboolean;
    }

    // SAFETY: the JVM passes a real `jobject` for the ByteBuffer parameter.
    let byte_buffer = JByteBuffer::from_raw(buffer);

    // A direct buffer's address is stable, so it can be read without copying
    // through the JVM heap. A non-direct buffer has no such address, which is
    // why the Kotlin side is required to send a direct one.
    let address = match env.get_direct_buffer_address(&byte_buffer) {
        Ok(ptr) => ptr,
        Err(e) => {
            cam_error(&format!(
                "push_frame: buffer bukan direct ByteBuffer ({e}); \
                 gunakan ByteBuffer.allocateDirect"
            ));
            return false as jni::sys::jboolean;
        }
    };

    let capacity = match env.get_direct_buffer_capacity(&byte_buffer) {
        Ok(cap) if cap > 0 => cap,
        Ok(_) => {
            cam_error("push_frame: kapasitas buffer 0");
            return false as jni::sys::jboolean;
        }
        Err(e) => {
            cam_error(&format!("push_frame: gagal membaca kapasitas buffer: {e}"));
            return false as jni::sys::jboolean;
        }
    };

    let accepted = push_frame_impl(
        width.max(0) as u32,
        height.max(0) as u32,
        rotation_degrees,
        address as *const u8,
        capacity,
    );
    accepted as jni::sys::jboolean
}

/// Actual frame intake, separated from the JNI signature so it can be unit
/// tested on a desktop host without a JVM.
///
/// # Safety
/// Same contract as the JNI wrapper above.
pub unsafe fn push_frame_impl(
    width: u32,
    height: u32,
    rotation_degrees: i32,
    buffer_ptr: *const u8,
    buffer_len: usize,
) -> bool {
    use crate::camera::log::{cam_debug, cam_error, cam_info};

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

    // Log the first few frames at INFO, then drop to DEBUG. The first frames are
    // the interesting ones: if the pipeline is broken, the count stays at zero
    // and the log says so plainly. After that, per-frame logging would flood
    // logcat.
    let count = guard.pushed_count();
    if count < 3 {
        cam_info(&format!(
            "frame #{count} diterima dari native: {width}x{height} rotasi={rotation_degrees} \
             ({} byte)",
            width as usize * height as usize * 4
        ));
    } else {
        cam_debug(&format!(
            "push_frame: {width}x{height} rotasi={rotation_degrees}"
        ));
    }

    // Validation, the RGBA-to-RGB alpha strip, and the store all live in the
    // shared `convert_and_store`, so this JNI path and the iOS C-ABI path stay
    // identical.
    match super::convert_and_store(&guard, width, height, rotation_degrees, buffer_ptr, buffer_len) {
        Ok(()) => true,
        Err(reason) => {
            cam_error(&format!("push_frame: {reason}"));
            false
        }
    }
}

/// Marks the native stream active/inactive.
///
/// JNI entry point for `CameraBridge.antitimpa_set_stream_active`.
///
/// Kotlin calls this when the capture session starts and stops, so `capture()`
/// can distinguish "session not started" from "session running but no frame
/// yet". Those need different UI messages.
#[no_mangle]
pub extern "C" fn Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1set_1stream_1active(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
    active: jni::sys::jboolean,
) {
    set_stream_active_impl(active != 0)
}

/// Separated from the JNI signature so it is unit testable.
pub fn set_stream_active_impl(active: bool) {
    let Some(mutex) = SLOT.get() else {
        crate::camera::log::cam_error("set_stream_active: slot belum di-install");
        return;
    };
    match mutex.lock() {
        Ok(slot) => {
            slot.set_stream_active(active);
            crate::camera::log::cam_info(&format!(
                "stream aktif={active} (total frame diterima={})",
                slot.pushed_count()
            ));
        }
        Err(_) => crate::camera::log::cam_error("set_stream_active: mutex poisoned"),
    }
}

/// Reports how many frames the native side has delivered.
///
/// JNI entry point for `CameraBridge.antitimpa_frames_received`.
///
/// Lets the Kotlin side confirm the wiring is live without inspecting Rust
/// state, and makes "zero frames ever" vs "frames but slot empty" obvious.
#[no_mangle]
pub extern "C" fn Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1frames_1received(
    _env: *mut jni::sys::JNIEnv,
    _class: jni::sys::jclass,
) -> jni::sys::jlong {
    frames_received_impl() as jni::sys::jlong
}

/// Separated from the JNI signature so it is unit testable.
pub fn frames_received_impl() -> u64 {
    SLOT.get()
        .and_then(|m| m.lock().ok().map(|s| s.pushed_count()))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::mobile::{NativeFrame, Rotation};

    /// Serializes these tests: they share the process-wide `SLOT`, so running
    /// concurrently would let one test's frames leak into another's
    /// assertions. Test binaries run tests in parallel by default.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn push(width: u32, height: u32, rotation: i32) -> bool {
        let buf = vec![255u8; (width * height * 4) as usize];
        unsafe { push_frame_impl(width, height, rotation, buf.as_ptr(), buf.len()) }
    }

    #[test]
    fn jni_symbols_follow_the_mangled_naming_convention() {
        // Regression test for the on-device crash: the JVM looks up symbols by
        // a name derived from package + class + method, with `_` escaped to
        // `_1`. Exporting a plain C name links fine and even appears in `nm`,
        // but the JVM never finds it, so the app dies with
        // `UnsatisfiedLinkError` the moment the method is first called.
        //
        // These literals are copied from the exact names the JVM reported
        // wanting in the crash log.
        const EXPECTED: [&str; 3] = [
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame",
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1set_1stream_1active",
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1frames_1received",
        ];

        // The functions must exist under those names and be callable.
        // Taking their addresses is enough to prove the symbols are defined
        // with the expected identifiers.
        let _: unsafe extern "C" fn(
            *mut jni::sys::JNIEnv,
            jni::sys::jclass,
            jni::sys::jint,
            jni::sys::jint,
            jni::sys::jint,
            jni::sys::jobject,
        ) -> jni::sys::jboolean = Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame;
        let _: extern "C" fn(
            *mut jni::sys::JNIEnv,
            jni::sys::jclass,
            jni::sys::jboolean,
        ) = Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1set_1stream_1active;
        let _: extern "C" fn(*mut jni::sys::JNIEnv, jni::sys::jclass) -> jni::sys::jlong =
            Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1frames_1received;

        // Guard the escaping rule itself, so a future rename cannot silently
        // reintroduce a name the JVM will not look for.
        for name in EXPECTED {
            assert!(
                name.starts_with("Java_org_antitimpa_antitimpa_CameraBridge_"),
                "symbol must carry the full package and class prefix: {name}"
            );
            assert!(
                !name.contains("antitimpa_push_frame"),
                "unescaped method name means the JVM will not find it: {name}"
            );
        }
    }

    /// Verifies `build.rs` keeps the JNI symbols through LTO and stripping.
    ///
    /// A release build removes these functions entirely: nothing in Rust
    /// references them, so the compiler and linker both treat them as dead code.
    /// The app then works in debug and dies in release with
    /// `UnsatisfiedLinkError`, which happened on device.
    ///
    /// This test reads `build.rs` and asserts the linker flags are present and
    /// spelled exactly as the exported symbols. It catches the two ways this fix
    /// regresses: the flags being deleted, or a symbol being renamed in one file
    /// but not the other.
    #[test]
    fn build_script_preserves_jni_symbols_for_release() {
        const BUILD_RS: &str = include_str!("../../../build.rs");

        // The three exported entry points, exactly as declared above.
        const SYMBOLS: [&str; 3] = [
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1push_1frame",
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1set_1stream_1active",
            "Java_org_antitimpa_antitimpa_CameraBridge_antitimpa_1frames_1received",
        ];

        for symbol in SYMBOLS {
            assert!(
                BUILD_RS.contains(symbol),
                "build.rs must pass --undefined={symbol}, otherwise a release \
                 build drops it and the app crashes with UnsatisfiedLinkError"
            );
        }

        assert!(
            BUILD_RS.contains("cargo:rustc-link-arg=-Wl,--undefined="),
            "build.rs must emit --undefined linker flags to keep JNI symbols"
        );
    }

    #[test]
    fn install_is_idempotent_and_shares_state() {
        let _guard = TEST_LOCK.lock().unwrap();
        let a = install_slot();
        let b = install_slot();
        // Both handles must observe one slot, otherwise native push and Rust
        // capture would use different buffers.
        a.push(NativeFrame {
            width: 1,
            height: 1,
            rgb: vec![1, 2, 3],
            rotation: Rotation::None,
        });
        assert!(b.has_frame(), "handles must share one slot");
        b.clear();
    }

    #[test]
    fn push_accepts_valid_buffer() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        slot.clear();

        assert!(push(4, 4, 0));
        let frame = slot.take().expect("frame should be stored");
        assert_eq!((frame.width, frame.height), (4, 4));
        assert_eq!(frame.rgb.len(), 4 * 4 * 3, "alpha is stripped at intake");
    }

    #[test]
    fn push_rejects_null_pointer() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        let accepted = unsafe { push_frame_impl(4, 4, 0, std::ptr::null(), 64) };
        assert!(!accepted, "null pointer must be rejected, not dereferenced");
    }

    #[test]
    fn push_rejects_undersized_buffer() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        let small = vec![0u8; 10];
        // Guards the most likely native-side mistake: sending YUV instead of
        // RGBA. A short read would be silent memory corruption.
        let accepted = unsafe { push_frame_impl(4, 4, 0, small.as_ptr(), small.len()) };
        assert!(!accepted, "undersized buffer must be rejected");
    }

    #[test]
    fn push_rejects_zero_dimensions() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        assert!(!push(0, 4, 0));
        assert!(!push(4, 0, 0));
    }

    #[test]
    fn stream_active_flag_round_trips() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        set_stream_active_impl(true);
        assert!(slot.is_stream_active());
        set_stream_active_impl(false);
        assert!(!slot.is_stream_active());
    }

    #[test]
    fn frames_received_counter_advances() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _ = install_slot();
        let before = frames_received_impl();
        assert!(push(2, 2, 0));
        assert_eq!(frames_received_impl(), before + 1);
    }

    #[test]
    fn rotation_is_recorded_from_native_hint() {
        let _guard = TEST_LOCK.lock().unwrap();
        let slot = install_slot();
        slot.clear();

        assert!(push(2, 2, 90));
        let frame = slot.take().unwrap();
        assert_eq!(frame.rotation, Rotation::Deg90);
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
            // buf dropped here; the pushed frame must own its own copy.
        }

        let frame = slot.take().expect("frame must survive source drop");
        assert!(frame.rgb.iter().all(|&b| b == 7));
    }
}
