import type { CameraDiagnostics } from "../types";

interface CameraPanelProps {
  info: CameraDiagnostics | null;
  busy: boolean;
  onCapture: () => void;
  disabled: boolean;
  /** When false, backend names, counters, and raw errors are hidden. */
  technical: boolean;
}

/**
 * Camera control surface.
 *
 * There is no `<video>` element: capture happens natively in Rust and only
 * metadata comes back, so there is nothing to stream into the DOM.
 *
 * Two presentations, one behaviour:
 *
 * * Simple (default). One button and a plain-language status. A user who just
 *   wants to check a QRIS should not have to read a device path.
 * * Technical. Backend name, capture counters, and the last raw error, which are
 *   what diagnose "the camera does not work".
 */
export function CameraPanel({ info, busy, onCapture, disabled, technical }: CameraPanelProps) {
  const isSynthetic = info?.synthetic ?? false;
  const neverCaptured = info != null && info.captures_ok === 0 && info.captures_failed === 0;
  const failed = (info?.captures_failed ?? 0) > 0 && info?.last_error;

  return (
    <div className="panel">
      {technical ? (
        <>
          <div className="row">
            <span className="row-label">Backend Kamera</span>
            <span className="row-value">
              {info ? info.backend : "memeriksa…"}
              {isSynthetic && <span className="tag">simulasi</span>}
            </span>
          </div>

          {info && (
            <div className="row">
              <span className="row-label">Frame</span>
              <span className="row-value">
                {info.captures_ok} berhasil
                {info.captures_failed > 0 && `, ${info.captures_failed} gagal`}
              </span>
            </div>
          )}

          {isSynthetic && (
            <div className="notice-inline">
              Tidak ada kamera asli terdeteksi. Frame sintetik dipakai agar pipeline
              analisis tetap bisa diuji.
            </div>
          )}

          {info?.last_error && (
            <div className="error">
              <strong>Error terakhir:</strong> {info.last_error}
            </div>
          )}

          {neverCaptured && !isSynthetic && (
            <p className="hint">
              Backend siap. Tekan tombol di bawah untuk mengambil satu frame.
            </p>
          )}
        </>
      ) : (
        <>
          {isSynthetic && (
            <p className="hint">
              Kamera tidak tersedia di perangkat ini. Pemindaian berjalan dalam mode
              simulasi.
            </p>
          )}
          {failed && (
            <p className="hint">Kamera bermasalah saat pengambilan terakhir. Coba lagi.</p>
          )}
        </>
      )}

      <button className="primary" onClick={onCapture} disabled={disabled || busy}>
        {busy ? "Memindai…" : "Pindai Kode QRIS"}
      </button>

      {technical && (
        <p className="hint hint-small">
          Diagnostik lengkap tercetak di terminal (<code>npm run tauri:dev</code>).
          Untuk log per-frame: <code>ANTITIMPA_CAM_DEBUG=1</code>.
        </p>
      )}
    </div>
  );
}
