/**
 * Thin typed wrapper around the Tauri IPC boundary.
 *
 * Every call into the Rust core goes through here so components never import
 * `@tauri-apps/api` directly. That keeps the IPC surface auditable and makes
 * it trivial to stub the backend for UI work in a plain browser.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  CameraDiagnostics,
  OpticalType,
  PreviewFrame,
  ScanSnapshot,
} from "./types";

/**
 * True when running inside the Tauri WebView rather than a bare browser.
 *
 * Checks several markers because the exact global differs between Tauri
 * versions and between `withGlobalTauri` on/off. A false negative here is
 * silent and confusing: the UI renders its "run in Tauri" placeholder while
 * the backend is perfectly reachable, so callers can never tell"not in Tauri"
 * from "IPC is broken".
 */
export const isTauri = (): boolean => {
  if (typeof window === "undefined") return false;
  const w = window as unknown as Record<string, unknown>;
  return (
    "__TAURI_INTERNALS__" in w ||
    "__TAURI__" in w ||
    "__TAURI_IPC__" in w
  );
};

/**
 * Runs Layer 2 (EMVCo) + Layer 3 (geofence) against a decoded payload.
 *
 * The CRC failure veto is applied in Rust: a failed checksum always yields a
 * combined score of 1.0 and a `HIGH RISK` band.
 */
export async function analyzePayload(
  payload: string,
  opticalType: OpticalType = "physical_camera_scan",
  clientCity?: string | null,
): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("analyze_payload", {
    payload,
    opticalType,
    clientCity: clientCity ?? null,
  });
}

/** Verifies only the CRC-16/CCITT-FALSE checksum of a payload. */
export async function verifyPayloadCrc(payload: string): Promise<boolean> {
  return invoke<boolean>("verify_payload_crc", { payload });
}

/** Runs Layer 3 alone, used by the manual client-city flow. */
export async function analyzeGeofence(
  clientCity: string | null,
  merchantCity: string | null,
): Promise<ScanSnapshot["l3"]> {
  return invoke<ScanSnapshot["l3"]>("analyze_geofence", {
    clientCity,
    merchantCity,
  });
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

/**
 * Reports which camera backend the Rust side opened, plus capture counters.
 *
 * Call this on mount and after every capture so the UI can show real state
 * instead of making the user read a terminal to find out what went wrong.
 */
export async function cameraDiagnostics(): Promise<CameraDiagnostics> {
  return invoke<CameraDiagnostics>("camera_diagnostics");
}

/**
 * Captures one frame and runs the full pipeline on it in Rust.
 *
 * Only metadata crosses the IPC boundary — the pixel buffer stays in Rust, so
 * this stays fast regardless of resolution.
 */
export async function captureAndAnalyze(
  opticalType: OpticalType = "physical_camera_scan",
  clientCity?: string | null,
): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("capture_and_analyze", {
    opticalType,
    clientCity: clientCity ?? null,
  });
}

/** Releases the camera device. Safe to call more than once. */
export async function releaseCamera(): Promise<void> {
  return invoke<void>("release_camera");
}

/**
 * Grabs one preview frame as a JPEG data URL.
 *
 * Call this on a timer from the preview component. Errors should be treated as
 * non-fatal: a dropped frame while the user is still positioning the camera is
 * normal, not something to surface as a failure.
 */
export async function cameraPreview(maxWidth?: number): Promise<PreviewFrame> {
  return invoke<PreviewFrame>("camera_preview", {
    maxWidth: maxWidth ?? null,
  });
}
