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
  ClientLocation,
  HistoryEntry,
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
 * the backend is perfectly reachable, so callers can never tell "not in Tauri"
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

/** Splits a location into the two optional arguments Rust expects. */
const fixArgs = (location?: ClientLocation | null) => ({
  clientLat: location?.lat ?? null,
  clientLon: location?.lon ?? null,
});

/**
 * Runs Layer 2 (EMVCo) + Layer 3 (geofence) against a decoded payload.
 *
 * Layer 1 cannot run here: there is no frame. The returned snapshot says so
 * explicitly via `coverage.optical_ran === false`, which the UI is expected to
 * surface rather than presenting the result as a complete check.
 *
 * The CRC failure veto is applied in Rust: a failed checksum always yields a
 * combined score of 1.0 and a `HIGH RISK` band.
 */
export async function analyzePayload(
  payload: string,
  opticalType: OpticalType = "physical_camera_scan",
  location?: ClientLocation | null,
): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("analyze_payload", {
    payload,
    opticalType,
    clientCity: location?.city ?? null,
    ...fixArgs(location),
  });
}

/** Verifies only the CRC-16/CCITT-FALSE checksum of a payload. */
export async function verifyPayloadCrc(payload: string): Promise<boolean> {
  return invoke<boolean>("verify_payload_crc", { payload });
}

/**
 * Runs Layer 3 alone, used by the manual client-city flow.
 *
 * Passing `clientLat`/`clientLon` enables the distance estimate, which is what
 * separates "different city nearby" from "different city, 1400 km away".
 */
export async function analyzeGeofence(
  clientCity: string | null,
  merchantCity: string | null,
  location?: ClientLocation | null,
): Promise<ScanSnapshot["l3"]> {
  return invoke<ScanSnapshot["l3"]>("analyze_geofence", {
    clientCity,
    merchantCity,
    ...fixArgs(location),
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
 * Captures a burst of frames and runs the full three-layer pipeline in Rust.
 *
 * A burst rather than one frame because Layer 1's temporal glare check needs a
 * series to tell a moving highlight from a static bright patch.
 *
 * Only metadata crosses the IPC boundary. The pixel buffers stay in Rust, so
 * this stays fast regardless of resolution.
 */
export async function captureAndAnalyze(
  opticalType: OpticalType = "physical_camera_scan",
  location?: ClientLocation | null,
  burstFrames?: number,
): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("capture_and_analyze", {
    opticalType,
    clientCity: location?.city ?? null,
    burstFrames: burstFrames ?? null,
    ...fixArgs(location),
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

/**
 * Analyses a raw RGB frame without a camera.
 *
 * This is the `imported_image` path: pixels arrive over IPC so Layer 1 can run
 * on them. Reserved for a real image-decoding flow; see the UI note about why
 * the current picker does not use it yet.
 */
export async function analyzeImageFrame(
  rgb: number[] | Uint8Array,
  width: number,
  height: number,
  bbox: [number, number, number, number] | null,
  opticalType: OpticalType = "imported_image",
  location?: ClientLocation | null,
): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("analyze_image_frame", {
    rgb: Array.from(rgb),
    width,
    height,
    bbox,
    opticalType,
    clientCity: location?.city ?? null,
    ...fixArgs(location),
  });
}

/**
 * Analyses an imported image file through all three layers, without a camera.
 *
 * Reserved for the validation path: this is how the Layer 1 thresholds get
 * checked against real photographs, and how Layer 1 can be demonstrated on a
 * machine with no camera and no printed sticker.
 *
 * Layer 1 runs with its spatial signals only. A single photo cannot support the
 * temporal glare check, and the result says so rather than reporting the absent
 * measurement as zero risk.
 */
export async function analyzeImageBytes(
  bytes: ArrayBuffer | Uint8Array,
  opticalType: OpticalType = "imported_image",
  location?: ClientLocation | null,
): Promise<ScanSnapshot> {
  return invoke<ScanSnapshot>("analyze_image_bytes", {
    bytes: Array.from(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)),
    opticalType,
    clientCity: location?.city ?? null,
    ...fixArgs(location),
  });
}

/** Metadata about an imported image, before running the analysis on it. */
export async function inspectImage(bytes: ArrayBuffer | Uint8Array): Promise<{
  original_width: number;
  original_height: number;
  width: number;
  height: number;
  downscaled: boolean;
  imported_frame_has_burst: boolean;
}> {
  return invoke("inspect_image", {
    bytes: Array.from(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)),
  });
}

// ---------------------------------------------------------------------------
// Offline geocoding
// ---------------------------------------------------------------------------

/**
 * Resolves a typed city name to coordinates using the bundled offline table.
 *
 * This is the desktop path: desktop has no OS location service, so Layer 3 would
 * otherwise always report "location unavailable". Returns `null` for an unknown
 * city, which Layer 3 reports as an unbounded mismatch rather than a fabricated
 * distance.
 */
export async function geocodeCity(city: string): Promise<[number, number] | null> {
  return invoke<[number, number] | null>("geocode_city", { city });
}

/** Every city in the offline table. */
export async function knownCities(): Promise<string[]> {
  return invoke<string[]>("known_cities");
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/** Records a completed scan into the tamper-evident chain. */
export async function recordScan(
  snapshot: ScanSnapshot,
  source: string,
  timestampMs?: number,
): Promise<HistoryEntry> {
  return invoke<HistoryEntry>("record_scan", {
    snapshot,
    source,
    timestampMs: timestampMs ?? null,
  });
}

/** Returns recorded scans, newest first. */
export async function historyEntries(): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("history_entries");
}

/**
 * Verifies the integrity chain over the recorded scans.
 *
 * Returns `[intact, detail]`. Exposed so the user can check that the evidence
 * list has not been altered.
 */
export async function historyVerify(): Promise<[boolean, string]> {
  return invoke<[boolean, string]>("history_verify");
}

export async function historyClear(): Promise<void> {
  return invoke<void>("history_clear");
}
