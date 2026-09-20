/**
 * Shared types mirroring the Rust `ScanSnapshot` / layer result structs.
 * Keeping these in sync is what lets the UI stay agnostic to which layer
 * produced a value.
 */

export type RiskLevel =
  | "NO QR"
  | "NOT RUN"
  | "MENUNGGU SCAN"
  | "LOW RISK"
  | "CAUTION"
  | "HIGH RISK";

export interface Layer1Result {
  l1_score: number;
  spatial_edge_density: number;
  temporal_glare_var: number;
  risk_level: RiskLevel;
}

export interface Layer2Result {
  l2_score: number;
  crc_valid: boolean;
  initiation_mode: string;
  mcc: string;
  merchant_name: string;
  merchant_city: string;
  parsed_tlv: Record<string, unknown>;
  warnings: string[];
}

export interface Layer3Result {
  l3_score: number;
  risk_level: RiskLevel;
  warnings: string[];
  client_city: string | null;
  merchant_city: string | null;
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
}

export type OpticalType = "physical_camera_scan" | "imported_image";

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
 * Raw pixels deliberately do not cross the IPC boundary: a 640x480 frame is
 * ~900 KB which becomes several MB as JSON, per frame. The Rust side downscales
 * and JPEG-encodes to roughly 40-80 KB, which is what makes a live preview
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
