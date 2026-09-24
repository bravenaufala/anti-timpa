import type { Finding } from "../types";

const SEVERITY_ORDER: Record<string, number> = { HIGH: 0, MEDIUM: 1, INFO: 2 };

const SEVERITY_LABEL: Record<string, string> = {
  HIGH: "Risiko tinggi",
  MEDIUM: "Perlu perhatian",
  INFO: "Informasi",
};

/** A ranked list of the named rules that fired. */
export function FindingsList({ findings }: { findings: Finding[] }) {
  if (findings.length === 0) return null;

  // The Rust side already orders worst-first; sorting again would only risk
  // the two orders drifting apart.
  const sorted = [...findings].sort(
    (a, b) => (SEVERITY_ORDER[a.severity] ?? 9) - (SEVERITY_ORDER[b.severity] ?? 9),
  );

  return (
    <div className="findings">
      <span className="row-label">Temuan</span>
      {sorted.map((f) => (
        <div key={f.code} className={`finding finding-${f.severity.toLowerCase()}`}>
          <div className="finding-head">
            <span className="finding-title">{f.title}</span>
            <span className="finding-badges">
              <span className="finding-layer">{f.layer}</span>
              <span className="finding-severity">{SEVERITY_LABEL[f.severity] ?? f.severity}</span>
            </span>
          </div>
          <p className="finding-detail">{f.detail}</p>
          <code className="finding-code">{f.code}</code>
        </div>
      ))}
    </div>
  );
}
