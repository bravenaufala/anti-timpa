//! C-ABI surface for embedding the Anti Timpa core in a host application.
//!
//! This is the seam a bank or payment-service-provider (PSP) would link
//! against to put the analysis inside their own app, without shipping Anti
//! Timpa as a separate product. It exposes the same core the app uses (a single
//! analyzer, [`crate::analyze_payload_snapshot`]), so an embedded copy and the
//! standalone app can never disagree.
//!
//! ## Status
//!
//! * Implemented and tested on the host: the functions below. They are
//!   ordinary `extern "C"` symbols, so a C/C++/Swift/Kotlin host can call them
//!   directly, and they are exercised by the unit tests at the bottom of this
//!   file.
//! * Scaffolding, not yet verified: the packaged SDK artifacts (`.aar` for
//!   Android, `.framework`/`.xcframework` for iOS). Producing those needs the
//!   NDK and Xcode respectively, which are not available in this build
//!   environment. See `sdk/README.md` and `sdk/generate-bindings.sh`.
//!
//! The ABI is JSON-in/JSON-out. A structured FFI type graph would have to be
//! re-declared in every host language; a string the host already knows how to
//! parse keeps the surface to three functions.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use crate::analyze_payload_snapshot;

/// Static, NUL-terminated version string.
const VERSION: &[u8] = b"anti-timpa-sdk/0.1.0\0";

/// Returns the SDK version as a static C string.
///
/// The pointer is to static storage and must not be passed to
/// [`antitimpa_free_string`].
#[no_mangle]
pub extern "C" fn antitimpa_version() -> *const c_char {
    VERSION.as_ptr() as *const c_char
}

/// Analyses a QRIS payload and returns the full scan snapshot as JSON.
///
/// * `payload`: NUL-terminated EMVCo payload. Required; NULL yields NULL.
/// * `client_city`: optional NUL-terminated city name, or NULL when the host
///   has no coarse location. This is what lets Layer 3 classify the location;
///   with NULL the geofence layer reports `NOT RUN` rather than a false "safe".
///
/// Layer 1 (optical) does not run here: there is no frame. The returned
/// snapshot says so via `coverage.optical_ran = false`, exactly as the app's
/// payload-only path does; the JSON never presents a partial check as complete.
///
/// Returns a heap-allocated, NUL-terminated UTF-8 JSON string that the caller
/// must release with [`antitimpa_free_string`], or NULL on invalid input.
///
/// # Safety
/// `payload` and `client_city` must each be either NULL or a valid
/// NUL-terminated C string. The string must remain valid for the call.
#[no_mangle]
pub unsafe extern "C" fn antitimpa_analyze_payload(
    payload: *const c_char,
    client_city: *const c_char,
) -> *mut c_char {
    if payload.is_null() {
        return std::ptr::null_mut();
    }

    // Reject invalid UTF-8 rather than lossy-decoding: a mangled payload would
    // otherwise be scored as the attacker's bytes, which NULL avoids.
    let payload = match CStr::from_ptr(payload).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return std::ptr::null_mut(),
    };
    let client_city = if client_city.is_null() {
        None
    } else {
        CStr::from_ptr(client_city).to_str().ok().map(str::to_string)
    };

    let snapshot = analyze_payload_snapshot(
        payload,
        Some("physical_camera_scan".to_string()),
        client_city,
        None,
        None,
    );

    match CString::new(serde_json::to_string(&snapshot).unwrap_or_else(|_| "null".to_string())) {
        Ok(json) => json.into_raw(),
        // A NUL in serialized JSON is impossible, but returning NULL beats an
        // unwrap that could abort the host process.
        Err(_) => std::ptr::null_mut(),
    }
}

/// Releases a string previously returned by [`antitimpa_analyze_payload`].
///
/// Passing NULL is a no-op, so callers do not have to guard the free.
///
/// # Safety
/// `ptr` must be NULL or a pointer returned by [`antitimpa_analyze_payload`]
/// that has not already been freed. Passing any other pointer is undefined
/// behaviour. Never free the result of [`antitimpa_version`].
#[no_mangle]
pub unsafe extern "C" fn antitimpa_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads and frees a string produced by the ABI, mirroring what a host does.
    unsafe fn take_string(ptr: *mut c_char) -> String {
        assert!(!ptr.is_null(), "ABI must not return NULL for valid input");
        let owned = CStr::from_ptr(ptr).to_str().unwrap().to_string();
        antitimpa_free_string(ptr);
        owned
    }

    #[test]
    fn version_is_a_readable_c_string() {
        let version = unsafe { CStr::from_ptr(antitimpa_version()) };
        assert_eq!(version.to_str().unwrap(), "anti-timpa-sdk/0.1.0");
    }

    #[test]
    fn null_payload_returns_null() {
        let out = unsafe { antitimpa_analyze_payload(std::ptr::null(), std::ptr::null()) };
        assert!(out.is_null(), "a NULL payload cannot be analysed");
    }

    #[test]
    fn free_accepts_null() {
        unsafe { antitimpa_free_string(std::ptr::null_mut()) };
    }

    #[test]
    fn analyze_returns_parseable_json_with_a_verdict() {
        let payload = CString::new("not-a-real-qris").unwrap();
        let out = unsafe { antitimpa_analyze_payload(payload.as_ptr(), std::ptr::null()) };
        let json = unsafe { take_string(out) };

        let value: serde_json::Value = serde_json::from_str(&json).expect("must be valid JSON");
        // A malformed payload fails CRC, which is a hard veto.
        assert_eq!(value["combined_risk_level"], "HIGH RISK");
        assert_eq!(value["scannable"], true);
        // Layer 1 did not run: the ABI must say so rather than imply a full check.
        assert_eq!(value["coverage"]["optical_ran"], false);
    }

    #[test]
    fn client_city_is_accepted_and_reaches_layer_3() {
        let payload = CString::new("not-a-real-qris").unwrap();
        let city = CString::new("BANDUNG").unwrap();
        let out = unsafe { antitimpa_analyze_payload(payload.as_ptr(), city.as_ptr()) };
        let json = unsafe { take_string(out) };

        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["l3"]["client_city"], "BANDUNG");
    }

    #[test]
    fn invalid_utf8_payload_is_rejected_not_lossy_decoded() {
        // 0xFF is never valid UTF-8. The ABI must return NULL, not a lossy
        // interpretation of an attacker-controlled byte sequence.
        let bytes = [0xFFu8, 0x00];
        let out = unsafe {
            antitimpa_analyze_payload(bytes.as_ptr() as *const c_char, std::ptr::null())
        };
        assert!(out.is_null());
    }
}
