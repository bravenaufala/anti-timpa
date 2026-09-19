import { useMemo } from "react";
import type { RiskLevel } from "../types";

/**
 * Visual risk band colors, keyed by the risk level strings emitted by Rust.
 * Kept as an explicit map so an unknown band fails visibly rather than silently.
 */
const RISK_COLORS: Record<string, string> = {
  "LOW RISK": "#2e7d32",
  CAUTION: "#f9a825",
  "HIGH RISK": "#c62828",
  "NO QR": "#546e7a",
  "NOT RUN": "#546e7a",
  "MENUNGGU SCAN": "#546e7a",
};

export const riskColor = (level: RiskLevel | string): string =>
  RISK_COLORS[level] ?? "#546e7a";

interface RiskGaugeProps {
  score: number;
  level: RiskLevel;
}

/** Semi-circular gauge showing the combined risk score. */
export function RiskGauge({ score, level }: RiskGaugeProps) {
  const color = riskColor(level);
  const percent = Math.round(Math.min(Math.max(score, 0), 1) * 100);

  // Arc geometry: 180 degrees sweep, drawn as a stroked circle path.
  const radius = 80;
  const circumference = Math.PI * radius;
  const dashOffset = useMemo(
    () => circumference * (1 - Math.min(Math.max(score, 0), 1)),
    [score, circumference],
  );

  return (
    <div className="gauge" role="img" aria-label={`Risiko ${level}, skor ${percent} persen`}>
      <svg viewBox="0 0 200 120" className="gauge-svg">
        <path
          d="M 20 110 A 80 80 0 0 1 180 110"
          fill="none"
          stroke="#263238"
          strokeWidth="14"
          strokeLinecap="round"
        />
        <path
          d="M 20 110 A 80 80 0 0 1 180 110"
          fill="none"
          stroke={color}
          strokeWidth="14"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          style={{ transition: "stroke-dashoffset 300ms ease, stroke 300ms ease" }}
        />
      </svg>
      <div className="gauge-readout">
        <span className="gauge-score" style={{ color }}>
          {score.toFixed(2)}
        </span>
        <span className="gauge-level" style={{ color }}>
          {level}
        </span>
      </div>
    </div>
  );
}
