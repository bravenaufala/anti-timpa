import type { Finding } from "../types";

const SEVERITY_ORDER: Record<string, number> = { HIGH: 0, MEDIUM: 1, INFO: 2 };

const SEVERITY_LABEL: Record<string, string> = {
  HIGH: "Risiko tinggi",
  MEDIUM: "Perlu perhatian",
  INFO: "Informasi",
};

interface FindingsListProps {
  findings: Finding[];
  /**
   * When false the layer badge and rule code are hidden, leaving only the
   * plain-language title and detail. The codes are what makes a finding
   * citable in a report; to a non-technical user they are just noise.
   */
  technical?: boolean;
}

/** A ranked list of the named rules that fired. */
export function FindingsList({ findings, technical = true }: FindingsListProps) {
  if (findings.length === 0) return null;

  // The Rust side already orders worst-first; sorting again would only risk
  // the two orders drifting apart.
  const sorted = [...findings].sort(
    (a, b) => (SEVERITY_ORDER[a.severity] ?? 9) - (SEVERITY_ORDER[b.severity] ?? 9),
  );

  return (
    <div className="findings">
      <span className="row-label">{technical ? "Temuan" : "Yang perlu diperhatikan"}</span>
      {sorted.map((f) => (
        <div key={f.code} className={`finding finding-${f.severity.toLowerCase()}`}>
          <div className="finding-head">
            <span className="finding-title">{f.title}</span>
            <span className="finding-badges">
              {technical && <span className="finding-layer">{f.layer}</span>}
              <span className="finding-severity">{SEVERITY_LABEL[f.severity] ?? f.severity}</span>
            </span>
          </div>
          {/* The detail is written for an engineer (rule names, thresholds,
              "hard veto"). In simple mode the severity chip plus the verdict
              advice already say what to do, so the jargon is left out. */}
          {technical && <p className="finding-detail">{f.detail}</p>}
          {technical && <code className="finding-code">{f.code}</code>}
        </div>
      ))}
    </div>
  );
}
