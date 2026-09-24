import { useCallback, useEffect, useState } from "react";
import type { HistoryEntry } from "../types";
import { historyClear, historyEntries, historyVerify } from "../api";

interface HistoryPanelProps {
  /** Bumped by the parent whenever a scan is recorded, to trigger a refetch. */
  revision: number;
  disabled?: boolean;
}

/**
 * Session scan history with a visible integrity check.
 *
 * The integrity button is not decoration: the chain is only meaningful if
 * someone can actually ask whether it still holds, and showing the raw link
 * hashes makes the mechanism inspectable rather than a claim in a document.
 */
export function HistoryPanel({ revision, disabled }: HistoryPanelProps) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [verdict, setVerdict] = useState<[boolean, string] | null>(null);
  const [error, setError] = useState<string | null>(null);

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
    try {
      await historyClear();
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const tone = (level: string) =>
    level === "HIGH RISK" ? "#ef5350" : level === "CAUTION" ? "#f9a825" : "#66bb6a";

  return (
    <section className="panel">
      <div className="history-head">
        <span className="row-label">Riwayat Pemindaian ({entries.length})</span>
        <div className="report-actions">
          <button className="sample" onClick={() => void check()} disabled={entries.length === 0}>
            Verifikasi rantai
          </button>
          <button className="sample" onClick={() => void clear()} disabled={entries.length === 0}>
            Bersihkan
          </button>
        </div>
      </div>

      <p className="hint hint-small">
        Disimpan di memori sesi saja, tidak pernah ke disk. Setiap entri
        di-hash bersama hash entri sebelumnya, sehingga penghapusan atau
        pengubahan entri akan terdeteksi.
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
        <p className="hint">Belum ada pemindaian yang direkam.</p>
      ) : (
        <div className="history-list">
          {entries.map((e) => (
            <div key={e.seq} className="history-row">
              <span className="history-seq">#{e.seq}</span>
              <span className="history-score" style={{ color: tone(e.combined_risk_level) }}>
                {e.combined_score.toFixed(2)}
              </span>
              <span className="history-band" style={{ color: tone(e.combined_risk_level) }}>
                {e.combined_risk_level}
              </span>
              <span className="history-preview" title={e.payload_preview}>
                {e.payload_preview || "—"}
              </span>
              <span className="history-layers">
                L1 {e.l1_score.toFixed(2)} · L2 {e.l2_score.toFixed(2)} · L3 {e.l3_score.toFixed(2)}
              </span>
              {e.top_finding && <span className="history-finding">{e.top_finding}</span>}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
