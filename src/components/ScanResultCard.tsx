import type { ScanSnapshot } from "../types";
import { friendlyBand } from "../labels";
import { FindingsList } from "./FindingsList";

interface ScanResultCardProps {
  snapshot: ScanSnapshot;
  busy: boolean;
}

/**
 * The result view for user-friendly mode.
 *
 * Two rules:
 *
 * 1. A failed read never shows a score. When `scannable` is false the card
 *    renders the reason and nothing else: no gauge, no merchant, no verdict.
 *    A number here would read as "checked and fine" for a scan that read
 *    nothing at all.
 * 2. A verdict never overstates itself. `LOW RISK` is worded as "no tampering
 *    found", not "safe", because a clean scan is an absence of evidence, not
 *    proof.
 */
export function ScanResultCard({ snapshot, busy }: ScanResultCardProps) {
  const failed = !snapshot.scannable;

  if (!failed && snapshot.combined_risk_level === "NO QR") {
    return (
      <section className="panel">
        <p className="hint">
          {busy ? "Memindai…" : "Arahkan kamera ke kode QRIS lalu tekan tombol pindai."}
        </p>
      </section>
    );
  }

  if (failed) {
    return (
      <section className="panel result-failed" role="status">
        <div className="result-failed-head">
          <span className="result-failed-icon" aria-hidden="true">
            !
          </span>
          <span className="result-failed-title">Hasil belum tersedia</span>
        </div>
        <p className="result-failed-text">
          {snapshot.error_reason ?? "Pemindaian tidak menghasilkan data QRIS."}
        </p>
        <p className="hint hint-small">
          Tidak ada nilai yang ditampilkan karena kode QRIS tidak berhasil dibaca.
        </p>
      </section>
    );
  }

  const band = friendlyBand(snapshot.combined_risk_level);
  const percent = Math.round(Math.min(Math.max(snapshot.combined_score, 0), 1) * 100);
  const merchant = snapshot.l2.merchant_name.trim();
  const city = snapshot.l2.merchant_city.trim();

  return (
    <section className="panel result-card">
      <div className="result-verdict" style={{ borderColor: band.color }}>
        <span className="result-verdict-badge" style={{ background: band.color }}>
          {band.short}
        </span>
        <span className="result-verdict-title" style={{ color: band.color }}>
          {band.title}
        </span>
      </div>

      <p className="result-advice">{band.advice}</p>

      <div className="result-facts">
        <div className="result-fact">
          <span className="row-label">Merchant</span>
          <span className="row-value result-fact-value">{merchant || "Tidak tercantum"}</span>
        </div>
        <div className="result-fact">
          <span className="row-label">Kota merchant</span>
          <span className="row-value result-fact-value">{city || "Tidak tercantum"}</span>
        </div>
        <div className="result-fact">
          <span className="row-label">Nilai risiko</span>
          <span className="row-value result-fact-value" style={{ color: band.color }}>
            {percent}%
          </span>
        </div>
      </div>

      <FindingsList findings={snapshot.findings} technical={false} />

      {/* The one partial-check warning kept in simple mode: if the optical
          layer did not run, the result cannot speak to a physically pasted
          sticker, which the app is meant to catch. The missing location
          comparison is an intentional simple-mode simplification and is only
          spelled out in technical mode. */}
      {!snapshot.coverage.optical_ran && (
        <p className="hint hint-small">
          Catatan: pemeriksaan tampilan fisik kode tidak dijalankan pada
          pemindaian ini. Pindai lewat kamera untuk melengkapinya.
        </p>
      )}
    </section>
  );
}
