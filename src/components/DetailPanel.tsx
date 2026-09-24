import type { Layer1Result, Layer2Result, Layer3Result, MismatchKind } from "../types";
import { riskColor } from "./RiskGauge";

interface DetailPanelProps {
  l1: Layer1Result;
  l2: Layer2Result;
  l3: Layer3Result;
  rawPayload: string;
  blurVar: number;
  isBlurry: boolean;
}

/** One row of the key/value detail list. */
function Row({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className="row">
      <span className="row-label">{label}</span>
      <span className="row-value" style={tone ? { color: tone } : undefined}>
        {value || "—"}
      </span>
    </div>
  );
}

/**
 * Human-readable meaning of each location-comparison outcome.
 *
 * Spelled out rather than shown as the raw enum because the distinction that
 * matters — "we compared and it differed" versus "we could not compare" — is
 * invisible in a score alone.
 */
const MISMATCH_LABELS: Record<MismatchKind, string> = {
  MATCH: "Cocok",
  SAME_METRO: "Wilayah yang sama",
  NOT_COMPARABLE: "Tidak dapat dibandingkan",
  NOT_EVALUATED: "Tidak dijalankan",
  DIFFERENT_CITY_NEARBY: "Kota berbeda, jarak dekat",
  DIFFERENT_CITY_DISTANT: "Kota berbeda, jarak jauh",
  DIFFERENT_CITY_UNBOUNDED: "Kota berbeda, jarak tidak diketahui",
};

/** Per-layer score chips plus the full EMVCo field breakdown. */
export function DetailPanel({
  l1,
  l2,
  l3,
  rawPayload,
  blurVar,
  isBlurry,
}: DetailPanelProps) {
  const allWarnings = [...l1.warnings, ...l2.warnings, ...l3.warnings];

  const distance =
    l3.distance_km === null ? "" : `${l3.distance_km.toFixed(0)} km`;

  return (
    <div className="panel">
      <div className="layer-chips">
        <span className="chip" style={{ borderColor: riskColor(l1.risk_level) }}>
          L1 {l1.l1_score.toFixed(2)}
          {l1.risk_level === "NOT RUN" ? " · tidak jalan" : ""}
        </span>
        <span
          className="chip"
          style={{ borderColor: riskColor(l2.crc_valid ? "LOW RISK" : "HIGH RISK") }}
        >
          L2 {l2.l2_score.toFixed(2)}
        </span>
        <span className="chip" style={{ borderColor: riskColor(l3.risk_level) }}>
          L3 {l3.l3_score.toFixed(2)}
          {!l3.evaluated ? " · tidak jalan" : ""}
        </span>
      </div>

      <Row label="Merchant" value={l2.merchant_name} />
      <Row label="Kota Merchant" value={l2.merchant_city} />
      <Row label="Kota Klien" value={l3.client_city ?? ""} />
      <Row
        label="Perbandingan Lokasi"
        value={MISMATCH_LABELS[l3.mismatch_kind] ?? l3.mismatch_kind}
        tone={l3.evaluated ? undefined : "#8b949e"}
      />
      {distance && <Row label="Jarak" value={distance} />}
      <Row label="MCC" value={l2.mcc} />
      <Row
        label="Mode Inisiasi"
        value={
          l2.initiation_mode === "12"
            ? "12 (Dinamis)"
            : l2.initiation_mode === "11"
              ? "11 (Statis)"
              : l2.initiation_mode
        }
      />
      <Row
        label="CRC-16"
        value={l2.crc_valid ? "VALID" : "GAGAL"}
        tone={l2.crc_valid ? "#66bb6a" : "#ef5350"}
      />
      <Row
        label="Blur (Laplacian)"
        value={blurVar.toFixed(1)}
        tone={isBlurry ? "#ef5350" : undefined}
      />

      {l1.risk_level !== "NOT RUN" && (
        <>
          <Row
            label="Tepi di quiet zone"
            value={l1.spatial_edge_density.toFixed(4)}
            tone={l1.spatial_edge_density > 0.065 ? "#f9a825" : "#66bb6a"}
          />
          <Row
            label="Kilau (glare) ring"
            value={`${(l1.glare_fraction * 100).toFixed(2)}%`}
          />
          <Row
            label="Variansi glare antar-frame"
            value={l1.temporal_glare_var.toFixed(6)}
          />
          <Row
            label="Quiet zone"
            value={l1.quiet_zone_truncated ? "terpotong (kurang andal)" : "utuh"}
            tone={l1.quiet_zone_truncated ? "#f9a825" : undefined}
          />
        </>
      )}

      <div className="row column">
        <span className="row-label">Payload</span>
        <code className="payload">{rawPayload || "—"}</code>
      </div>

      {allWarnings.length > 0 && (
        <div className="warnings">
          <span className="row-label">Catatan Lapisan</span>
          {allWarnings.map((w, i) => (
            <div key={`${i}-${w}`} className="warning-item">
              {w}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
