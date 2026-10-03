/**
 * Shared types mirroring the Rust `ScanSnapshot` / layer result structs.
 * Keeping these in sync is what lets the UI stay agnostic to which layer
 * produced a value.
 */

export type RiskLevel =
  | "NO QR"
  | "NOT RUN"
  | "NOT COMPARABLE"
  | "MENUNGGU SCAN"
  | "LOW RISK"
  | "CAUTION"
  | "HIGH RISK";

export interface Layer1Result {
  l1_score: number;
  spatial_edge_density: number;
  temporal_glare_var: number;
  /**
   * Peak local texture variance. Reported for calibration but given no weight
   * in the score: it saturates on any readable QR, so it cannot discriminate a
   * tampered symbol from a clean one.
   */
  texture_discontinuity: number;
  /** Share of the detection ring covered by specular glare. */
  glare_fraction: number;
  risk_level: RiskLevel;
  /** True when the QR sits too close to the frame edge to measure its margin. */
  quiet_zone_truncated: boolean;
  warnings: string[];
}

export interface Layer2Result {
  l2_score: number;
  crc_valid: boolean;
  initiation_mode: string;
  mcc: string;
  merchant_name: string;
  merchant_city: string;
  /** National Merchant ID (NMID) from the merchant account sub-TLVs, if present. */
  merchant_id: string;
  parsed_tlv: Record<string, unknown>;
  warnings: string[];
}

/** How the location comparison actually resolved. */
export type MismatchKind =
  | "MATCH"
  | "SAME_METRO"
  | "NOT_COMPARABLE"
  | "NOT_EVALUATED"
  | "DIFFERENT_CITY_NEARBY"
  | "DIFFERENT_CITY_DISTANT"
  | "DIFFERENT_CITY_UNBOUNDED";

export interface Layer3Result {
  l3_score: number;
  risk_level: RiskLevel;
  warnings: string[];
  client_city: string | null;
  merchant_city: string | null;
  mismatch_kind: MismatchKind;
  distance_km: number | null;
  location_available: boolean;
  /** False when the comparison was skipped, which is not the same as "passed". */
  evaluated: boolean;
}

/** Which layers actually executed for a scan. */
export interface ScanCoverage {
  optical_ran: boolean;
  payload_ran: boolean;
  geofence_ran: boolean;
  /** True only when every layer that could apply actually ran. */
  complete: boolean;
  summary: string;
}

/** A single named risk rule that fired. */
export interface Finding {
  /** Stable rule identifier, e.g. `L2_CRC_MISMATCH`. */
  code: string;
  layer: string;
  severity: "HIGH" | "MEDIUM" | "INFO" | string;
  title: string;
  detail: string;
}

export interface ScanSnapshot {
  l1: Layer1Result;
  l2: Layer2Result;
  l3: Layer3Result;
  combined_score: number;
  combined_risk_level: RiskLevel;
  is_blurry: boolean;
  blur_var: number;
  qr_bbox: [number, number, number, number] | null;
  raw_qris_str: string;
  /** Set when no QR was read; explains what the user should do next. */
  no_qr_reason: string | null;
  /**
   * Whether this snapshot carries a result worth showing at all.
   *
   * `false` for a failed read: no symbol found, a frame too blurry to decode,
   * or a camera error. When `false`, the UI must show only `error_reason` and
   * must never render a score.
   */
  scannable: boolean;
  /** Short, non-technical reason no result is available; `null` when scannable. */
  error_reason: string | null;
  coverage: ScanCoverage;
  findings: Finding[];
  chain_hash: number | null;
}

export type OpticalType = "physical_camera_scan" | "imported_image";

/** One recorded scan, as stored in the Rust-side hash chain. */
export interface HistoryEntry {
  seq: number;
  timestamp_ms: number | null;
  source: string;
  combined_score: number;
  combined_risk_level: string;
  /** Merchant name from Tag 59, so history is readable without decoding a payload. */
  merchant_name: string;
  merchant_city: string;
  payload_preview: string;
  l1_score: number;
  l2_score: number;
  l3_score: number;
  crc_valid: boolean;
  top_finding: string | null;
  prev_hash: number;
  entry_hash: number;
}

/** Which camera backend Rust actually opened, plus runtime health. */
export interface CameraDiagnostics {
  backend: string;
  ready: boolean;
  /** True when frames are generated rather than captured. */
  synthetic: boolean;
  /** Successful frame grabs since startup. */
  captures_ok: number;
  /** Failed frame grabs since startup. */
  captures_failed: number;
  /** Last error message, cleared on a successful capture. */
  last_error: string | null;
}

/**
 * One preview frame, already JPEG-encoded as a data URL.
 *
 * Raw pixels do not cross the IPC boundary: a 640x480 frame is ~900 KB which
 * becomes several MB as JSON, per frame. The Rust side downscales and
 * JPEG-encodes to roughly 40-80 KB, which is what makes a live preview
 * affordable.
 */
export interface PreviewFrame {
  data_url: string;
  /** Dimensions after downscaling, for aspect-ratio handling. */
  width: number;
  height: number;
  /** Encoded payload size in bytes. */
  byte_len: number;
}

/** A coarse device position, supplied to Layer 3 for the distance estimate. */
export interface ClientLocation {
  city: string | null;
  lat: number | null;
  lon: number | null;
}
