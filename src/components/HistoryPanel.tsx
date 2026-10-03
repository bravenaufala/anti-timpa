import { useCallback, useEffect, useState } from "react";
import type { HistoryEntry } from "../types";
import { historyClear, historyEntries, historyVerify } from "../api";
import { formatTimestamp, friendlyBand } from "../labels";

interface HistoryPanelProps {
  /** Bumped by the parent whenever a scan is recorded, to trigger a refetch. */
  revision: number;
  disabled?: boolean;
  /** When false, chain hashes, layer scores, and verification are hidden. */
  technical: boolean;
}

/**
 * Locally saved scan history ("riwayat pemindaian").
 *
 * Entries are written automatically after every scan that produced a result, by
 * the parent. This panel only reads and clears them; there is no export or
 * download, matching the product decision that a text list is enough.
 *
 * The integrity check is a technical-mode feature: the chain is only meaningful
 * if someone can ask whether it still holds, but a raw link hash is noise to a
 * user who just wants to see what they scanned.
 */
export function HistoryPanel({ revision, disabled, technical }: HistoryPanelProps) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [verdict, setVerdict] = useState<[boolean, string] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setEntries(await historyEntries());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    if (!disabled) void refresh();
  }, [refresh, revision, disabled]);

  const check = async () => {
    setError(null);
    try {
      setVerdict(await historyVerify());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const clear = async () => {
    setError(null);
    setVerdict(null);
    setConfirming(false);
    try {
      await historyClear();
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="panel">
      <div className="history-head">
        <span className="row-label">Riwayat Pemindaian ({entries.length})</span>
        <div className="report-actions">
          {technical && (
            <button className="sample" onClick={() => void check()} disabled={entries.length === 0}>
              Verifikasi rantai
            </button>
          )}
          {entries.length > 0 &&
            (confirming ? (
              <>
                <button className="sample" onClick={() => void clear()}>
                  Hapus semua
                </button>
                <button className="sample" onClick={() => setConfirming(false)}>
                  Batal
                </button>
              </>
            ) : (
              <button className="sample" onClick={() => setConfirming(true)}>
                Bersihkan
              </button>
            ))}
        </div>
      </div>

      <p className="hint hint-small">
        Setiap hasil pemindaian tersimpan otomatis di perangkat ini dan tidak
        pernah dikirim ke mana pun.
      </p>

      {verdict && (
        <div className={verdict[0] ? "coverage coverage-complete" : "coverage coverage-partial"}>
          <span className="coverage-icon" aria-hidden="true">
            {verdict[0] ? "✓" : "!"}
          </span>
          <span>
            {verdict[0] ? "Rantai utuh: " : "Rantai bermasalah: "}
            {verdict[1]}
          </span>
        </div>
      )}

      {error && <div className="error">{error}</div>}

      {entries.length === 0 ? (
        <p className="hint">Belum ada pemindaian yang tersimpan.</p>
      ) : (
        <div className="history-list">
          {entries.map((e) => {
            const band = friendlyBand(e.combined_risk_level);
            const title = e.merchant_name.trim();
            return (
              <div key={e.seq} className="history-row">
                <span className="history-time">{formatTimestamp(e.timestamp_ms)}</span>
                <span className="history-band" style={{ color: band.color }}>
                  {band.short}
                </span>
                <span className="history-merchant">{title || "Merchant tidak tercantum"}</span>
                <span className="history-sub">
                  {e.merchant_city.trim() ? `${e.merchant_city} · ` : ""}
                  nilai risiko {Math.round(e.combined_score * 100)}%
                </span>
                {technical && (
                  <>
                    <span className="history-seq">
                      #{e.seq} ·{e.source}
                    </span>
                    <span className="history-layers">
                      L1 {e.l1_score.toFixed(2)} · L2 {e.l2_score.toFixed(2)} · L3{" "}
                      {e.l3_score.toFixed(2)} · CRC {e.crc_valid ? "valid" : "gagal"}
                    </span>
                    {e.top_finding && <span className="history-finding">{e.top_finding}</span>}
                    <span className="history-preview" title={e.payload_preview}>
                      {e.payload_preview || "—"}
                    </span>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
