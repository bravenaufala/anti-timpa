import type { CameraDiagnostics } from "../types";

interface CameraPanelProps {
  info: CameraDiagnostics | null;
  busy: boolean;
  onCapture: () => void;
  disabled: boolean;
}

/**
 * Camera control surface with live diagnostics.
 *
 * Deliberately has no `<video>` element: capture happens natively in Rust and
 * only metadata comes back, so there is nothing to stream into the DOM. That is
 * the key architectural difference from the old Kivy app, which had to render
 * frames through a widget and then read pixels back out of it.
 *
 * The counters are shown because "the camera does not work" is otherwise
 * impossible to act on. Success/failure counts and the last error distinguish
 * the real causes: device missing, permission denied, device busy, or a native
 * bridge that never delivered a frame.
 */
export function CameraPanel({ info, busy, onCapture, disabled }: CameraPanelProps) {
  const isSynthetic = info?.synthetic ?? false;
  const neverCaptured = info != null && info.captures_ok === 0 && info.captures_failed === 0;

  return (
    <div className="panel">
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

      <button className="primary" onClick={onCapture} disabled={disabled || busy}>
        {busy ? "Mengambil & menganalisis…" : "Ambil Foto QR (One-Shot)"}
      </button>

      <p className="hint hint-small">
        Diagnostik lengkap tercetak di terminal (<code>npm run tauri:dev</code>).
        Untuk log per-frame: <code>ANTITIMPA_CAM_DEBUG=1</code>.
      </p>
    </div>
  );
}
