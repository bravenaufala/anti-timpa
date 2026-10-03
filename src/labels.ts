/**
 * Human-readable presentation of the risk bands emitted by Rust.
 *
 * Kept in one place so the friendly result card and the history list never
 * drift apart, and so a new band fails visibly (falls back to a neutral label)
 * instead of leaving a blank verdict.
 */

import type { RiskLevel } from "./types";

/** Band colors, shared by the gauge, the detail rows, and the history list. */
const RISK_COLORS: Record<string, string> = {
  "LOW RISK": "#2e7d32",
  CAUTION: "#f9a825",
  "HIGH RISK": "#c62828",
  "NO QR": "#546e7a",
  "NOT RUN": "#546e7a",
  "NOT COMPARABLE": "#546e7a",
  "MENUNGGU SCAN": "#546e7a",
};

export const riskColor = (level: RiskLevel | string): string => RISK_COLORS[level] ?? "#546e7a";

export interface BandLabel {
  /** Short verdict a non-technical user can act on. */
  title: string;
  /** One sentence saying what it means. */
  advice: string;
  /** Lower-case label for a history row. */
  short: string;
  color: string;
}

/**
 * Maps a band to plain Indonesian.
 *
 * Avoids "LOW RISK = aman": the scan found no anomaly, which is weaker than a
 * guarantee. The wording keeps that distinction.
 */
export function friendlyBand(level: RiskLevel | string): BandLabel {
  switch (level) {
    case "LOW RISK":
      return {
        title: "Tidak ditemukan indikasi pemalsuan",
        advice:
          "Struktur kode dan tampilan fisiknya wajar. Tetap cocokkan nama merchant dengan yang Anda tuju sebelum membayar.",
        short: "Aman",
        color: RISK_COLORS["LOW RISK"],
      };
    case "CAUTION":
      return {
        title: "Perlu diperiksa lebih teliti",
        advice:
          "Ada hal yang tidak biasa pada kode ini. Periksa nama merchant dan tanyakan ke petugas sebelum membayar.",
        short: "Perlu periksa",
        color: RISK_COLORS.CAUTION,
      };
    case "HIGH RISK":
      return {
        title: "Berisiko tinggi — jangan dibayar dulu",
        advice:
          "Kode ini menunjukkan tanda pemalsuan atau kerusakan data. Jangan lanjutkan pembayaran sebelum dipastikan.",
        short: "Berisiko",
        color: RISK_COLORS["HIGH RISK"],
      };
    default:
      return {
        title: "Belum ada hasil",
        advice: "Pindai kode QRIS untuk melihat hasilnya.",
        short: level,
        color: RISK_COLORS["NO QR"],
      };
  }
}

/** Formats a stored timestamp for the history list. */
export function formatTimestamp(ms: number | null): string {
  if (ms === null) return "waktu tidak tersedia";
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return "waktu tidak tersedia";
  return d.toLocaleString("id-ID", {
    day: "2-digit",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}
