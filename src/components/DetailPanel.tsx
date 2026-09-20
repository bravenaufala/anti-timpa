import type { Layer1Result, Layer2Result, Layer3Result } from "../types";
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

/** Per-layer score chips plus the full EMVCo field breakdown. */
export function DetailPanel({
  l1,
  l2,
  l3,
  rawPayload,
  blurVar,
  isBlurry,
}: DetailPanelProps) {
  const allWarnings = [...l2.warnings, ...l3.warnings];

  return (
    <div className="panel">
      <div className="layer-chips">
        <span className="chip" style={{ borderColor: riskColor(l1.risk_level) }}>
          L1 {l1.l1_score.toFixed(2)}
        </span>
        <span className="chip" style={{ borderColor: riskColor(l2.crc_valid ? "LOW RISK" : "HIGH RISK") }}>
          L2 {l2.l2_score.toFixed(2)}
        </span>
        <span className="chip" style={{ borderColor: riskColor(l3.risk_level) }}>
          L3 {l3.l3_score.toFixed(2)}
        </span>
      </div>

      <Row label="Merchant" value={l2.merchant_name} />
      <Row label="Kota Merchant" value={l2.merchant_city} />
      <Row label="Kota Klien" value={l3.client_city ?? ""} />
      <Row label="MCC" value={l2.mcc} />
      <Row
        label="Mode Inisiasi"
        value={l2.initiation_mode === "12" ? "12 (Dinamis)" : l2.initiation_mode === "11" ? "11 (Statis)" : l2.initiation_mode}
      />
      <Row
        label="CRC-16"
        value={l2.crc_valid ? "VALID" : "GAGAL"}
        tone={l2.crc_valid ? "#66bb6a" : "#ef5350"}
      />
      <Row label="Blur (Laplacian)" value={blurVar.toFixed(1)} tone={isBlurry ? "#ef5350" : undefined} />

      <div className="row column">
        <span className="row-label">Payload</span>
        <code className="payload">{rawPayload || "—"}</code>
      </div>

      {allWarnings.length > 0 && (
        <div className="warnings">
          <span className="row-label">Peringatan</span>
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
