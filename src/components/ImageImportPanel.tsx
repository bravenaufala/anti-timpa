import { useRef, useState } from "react";
import type { ClientLocation, OpticalType, ScanSnapshot } from "../types";
import { analyzeImageBytes } from "../api";

interface ImageImportPanelProps {
  busy: boolean;
  opticalType: OpticalType;
  location: ClientLocation;
  onResult: (snapshot: ScanSnapshot, source: string) => void;
}

/**
 * Analyse a photo instead of using the camera.
 *
 * Framed in the UI as what it actually is — a way to run the optical layer
 * without a camera — rather than as a headline feature. Two reasons:
 *
 * 1. The overlay attack happens at a physical QR in front of a camera, so the
 *    camera path is the one that addresses it. Presenting import as equivalent
 *    would misrepresent the product.
 * 2. An imported photo cannot support the temporal glare check, because there is
 *    no burst. The panel says so up front so the resulting coverage warning is
 *    not a surprise.
 */
export function ImageImportPanel({
  busy,
  opticalType,
  location,
  onResult,
}: ImageImportPanelProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [fileName, setFileName] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);

  const onPick = async (file: File) => {
    setError(null);
    setWorking(true);
    setFileName(file.name);
    // An object URL is cheaper than a data URL for the preview and is revoked
    // below; it never leaves the page.
    setPreview(URL.createObjectURL(file));

    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const snapshot = await analyzeImageBytes(bytes, opticalType, location);
      onResult(snapshot, "imported_image");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setWorking(false);
    }
  };

  const reset = () => {
    if (preview) URL.revokeObjectURL(preview);
    setPreview(null);
    setFileName(null);
    setError(null);
    if (inputRef.current) inputRef.current.value = "";
  };

  return (
    <section className="panel">
      <span className="row-label">Analisis dari Berkas Gambar</span>
      <p className="hint hint-small">
        Untuk menguji lapisan optik tanpa kamera. Foto yang diimpor tidak bisa
        menjalankan pemeriksaan kilau antar-frame, karena itu membutuhkan
        beberapa frame berurutan — hasilnya akan menandai bagian itu sebagai
        tidak dijalankan.
      </p>

      <input
        ref={inputRef}
        className="input"
        type="file"
        accept="image/*"
        disabled={busy || working}
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) void onPick(f);
        }}
      />

      {(working || busy) && <p className="hint hint-small">Menganalisis gambar…</p>}

      {preview && (
        <div className="import-preview">
          <img className="import-img" src={preview} alt={fileName ?? "gambar diimpor"} />
          <div className="import-meta">
            <span>{fileName}</span>
            <button className="sample" onClick={reset} disabled={working || busy}>
              Bersihkan
            </button>
          </div>
        </div>
      )}

      {error && <div className="error">{error}</div>}
    </section>
  );
}
