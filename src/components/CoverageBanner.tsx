import type { ScanCoverage } from "../types";

/**
 * States which layers ran and which did not.
 *
 * This component exists to close a specific failure mode: a scan that only
 * checked the payload can come back `LOW RISK`, because a sticker attack leaves
 * the payload byte-identical to the original. Showing a green verdict without
 * saying "the optical layer did not run" would tell the user their QR is clean
 * when the one check that could have caught a physical overlay never executed.
 *
 * So a partial scan is never rendered as a plain success: it gets a warning
 * banner naming exactly what was skipped.
 */
export function CoverageBanner({ coverage }: { coverage: ScanCoverage }) {
  if (!coverage.summary) return null;

  if (coverage.complete) {
    return (
      <div className="coverage coverage-complete">
        <span className="coverage-icon" aria-hidden="true">
          ✓
        </span>
        <span>{coverage.summary}</span>
      </div>
    );
  }

  const skipped: string[] = [];
  if (!coverage.optical_ran) skipped.push("analisis optik (L1)");
  if (!coverage.payload_ran) skipped.push("analisis payload (L2)");
  if (!coverage.geofence_ran) skipped.push("perbandingan lokasi (L3)");

  return (
    <div className="coverage coverage-partial" role="status">
      <span className="coverage-icon" aria-hidden="true">
        !
      </span>
      <div className="coverage-body">
        <strong>Pemeriksaan tidak lengkap</strong>
        <p className="coverage-text">{coverage.summary}</p>
        {skipped.length > 0 && (
          <p className="coverage-text coverage-skipped">
            Tidak diperiksa: {skipped.join(", ")}.
          </p>
        )}
        {!coverage.optical_ran && (
          <p className="coverage-text">
            Layer optik adalah satu-satunya lapisan yang bisa mendeteksi QR yang
            ditempeli stiker — payload QR yang ditempeli tidak berubah, sehingga
            CRC dan struktur tetap valid. Hasil di bawah ini <em>tidak</em>{" "}
            mencakup pemeriksaan tersebut.
          </p>
        )}
      </div>
    </div>
  );
}
