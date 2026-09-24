import { useState } from "react";
import type { ScanSnapshot } from "../types";
import { generateReport } from "../api";

interface ReportPanelProps {
  snapshot: ScanSnapshot;
  /** Where the payload came from: `camera`, `manual`, or `sample`. */
  source: string;
  disabled?: boolean;
}

/**
 * On-device report generation.
 *
 * The report is produced by the Rust core and displayed here. Nothing is
 * uploaded: downloading or copying is the user's action, on their machine,
 * which is what keeps this feature compatible with the app's "100% local"
 * guarantee even though its purpose is to share a finding.
 */
export function ReportPanel({ snapshot, source, disabled }: ReportPanelProps) {
  const [format, setFormat] = useState<"text" | "html">("text");
  const [body, setBody] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const build = async () => {
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      const text = await generateReport(snapshot, source, format, Date.now());
      setBody(text);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const copy = async () => {
    if (!body) return;
    try {
      await navigator.clipboard.writeText(body);
      setCopied(true);
    } catch (e) {
      setError(
        `Gagal menyalin ke clipboard: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  };

  const download = () => {
    if (!body) return;
    const isHtml = format === "html";
    const blob = new Blob([body], {
      type: isHtml ? "text/html;charset=utf-8" : "text/plain;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `laporan-qris-${Date.now()}.${isHtml ? "html" : "txt"}`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <section className="panel">
      <span className="row-label">Laporan Bukti</span>
      <p className="hint hint-small">
        Dibuat di perangkat ini. Tidak ada data yang dikirim ke mana pun —
        menyalin atau mengunduh adalah tindakan Anda sendiri.
      </p>

      <div className="report-controls">
        <label className="field">
          <span className="row-label">Format</span>
          <select
            className="input"
            value={format}
            onChange={(e) => {
              setFormat(e.target.value as "text" | "html");
              setBody(null);
            }}
          >
            <option value="text">Teks (untuk chat/email)</option>
            <option value="html">HTML (untuk cetak/PDF)</option>
          </select>
        </label>
        <button className="primary" disabled={busy || disabled} onClick={() => void build()}>
          {busy ? "Membuat…" : "Buat laporan"}
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      {body && (
        <>
          <div className="report-actions">
            <button className="sample" onClick={() => void copy()}>
              {copied ? "Tersalin ✓" : "Salin"}
            </button>
            <button className="sample" onClick={download}>
              Unduh .{format === "html" ? "html" : "txt"}
            </button>
          </div>
          {format === "html" ? (
            <p className="hint hint-small">
              Pratinjau HTML di bawah. Buka berkas yang diunduh untuk tampilan
              cetak.
            </p>
          ) : (
            <pre className="report-body">{body}</pre>
          )}
          {format === "html" && (
            <pre className="report-body report-body-html">{body}</pre>
          )}
        </>
      )}
    </section>
  );
}
