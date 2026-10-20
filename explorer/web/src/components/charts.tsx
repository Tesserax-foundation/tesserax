import { useId, useState } from "react";

export interface Point {
  label: string;
  value: number;
  /** Shown in the tooltip instead of the value. */
  display?: string;
}

const WIDTH = 640;
const HEIGHT = 200;
const PAD = { top: 12, right: 8, bottom: 24, left: 8 };

function useHover() {
  const [hover, setHover] = useState<number | null>(null);
  return { hover, setHover };
}

function Tooltip({ point, x }: { point: Point; x: number }) {
  return (
    <div
      className="pointer-events-none absolute top-1 z-10 -translate-x-1/2 whitespace-nowrap rounded-lg border border-line bg-surface px-2.5 py-1.5 text-xs shadow-lg"
      style={{ left: `${(x / WIDTH) * 100}%` }}
    >
      <div className="text-muted">{point.label}</div>
      <div className="font-semibold tabular">{point.display ?? point.value.toLocaleString("en-US")}</div>
    </div>
  );
}

/** Bars, one per point, with a hover tooltip. */
export function BarChart({ points, height = HEIGHT }: { points: Point[]; height?: number }) {
  const { hover, setHover } = useHover();
  if (points.length === 0) return <div className="flex h-[200px] items-center justify-center text-sm text-muted">No data yet</div>;
  const max = Math.max(...points.map((p) => p.value), 1);
  const inner = WIDTH - PAD.left - PAD.right;
  const step = inner / points.length;
  const bar = Math.max(2, step * 0.7);
  const y = (v: number) => PAD.top + (height - PAD.top - PAD.bottom) * (1 - v / max);
  return (
    <div className="relative">
      {hover != null && <Tooltip point={points[hover]} x={PAD.left + step * hover + step / 2} />}
      <svg viewBox={`0 0 ${WIDTH} ${height}`} className="h-auto w-full" role="img" onMouseLeave={() => setHover(null)}>
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1={PAD.left} x2={WIDTH - PAD.right} y1={y(max * f)} y2={y(max * f)} stroke="var(--border)" strokeDasharray="3 4" />
        ))}
        {points.map((p, i) => {
          const x = PAD.left + step * i + (step - bar) / 2;
          return (
            <g key={i} onMouseEnter={() => setHover(i)}>
              <rect x={PAD.left + step * i} y={PAD.top} width={step} height={height - PAD.top - PAD.bottom} fill="transparent" />
              <rect x={x} y={y(p.value)} width={bar} height={Math.max(1, height - PAD.bottom - y(p.value))} rx={Math.min(3, bar / 3)} fill={hover === i ? "var(--accent-strong)" : "var(--accent)"} opacity={hover == null || hover === i ? 1 : 0.55} />
            </g>
          );
        })}
        <Labels points={points} step={step} height={height} />
      </svg>
    </div>
  );
}

/** A line with its area, with a hover tooltip. */
export function LineChart({ points, height = HEIGHT }: { points: Point[]; height?: number }) {
  const { hover, setHover } = useHover();
  const gradient = useId();
  if (points.length === 0) return <div className="flex h-[200px] items-center justify-center text-sm text-muted">No data yet</div>;
  const values = points.map((p) => p.value);
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || max || 1;
  const inner = WIDTH - PAD.left - PAD.right;
  const step = points.length > 1 ? inner / (points.length - 1) : 0;
  const x = (i: number) => PAD.left + (points.length > 1 ? step * i : inner / 2);
  const y = (v: number) => PAD.top + (height - PAD.top - PAD.bottom) * (1 - (v - min + span * 0.1) / (span * 1.2));
  const line = points.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p.value).toFixed(1)}`).join(" ");
  const area = `${line} L${x(points.length - 1)},${height - PAD.bottom} L${x(0)},${height - PAD.bottom} Z`;
  return (
    <div className="relative">
      {hover != null && <Tooltip point={points[hover]} x={x(hover)} />}
      <svg viewBox={`0 0 ${WIDTH} ${height}`} className="h-auto w-full" role="img" onMouseLeave={() => setHover(null)}>
        <defs>
          <linearGradient id={gradient} x1="0" x2="0" y1="0" y2="1">
            <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.28" />
            <stop offset="100%" stopColor="var(--accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        <path d={area} fill={`url(#${gradient})`} />
        <path d={line} fill="none" stroke="var(--accent)" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
        {points.map((_, i) => (
          <rect key={i} x={x(i) - Math.max(step, 8) / 2} y={0} width={Math.max(step, 8)} height={height} fill="transparent" onMouseEnter={() => setHover(i)} />
        ))}
        {hover != null && (
          <>
            <line x1={x(hover)} x2={x(hover)} y1={PAD.top} y2={height - PAD.bottom} stroke="var(--border-strong)" />
            <circle cx={x(hover)} cy={y(points[hover].value)} r={4} fill="var(--surface)" stroke="var(--accent)" strokeWidth={2} />
          </>
        )}
        <Labels points={points} step={points.length > 1 ? step : inner} height={height} centered={false} />
      </svg>
    </div>
  );
}

function Labels({ points, step, height, centered = true }: { points: Point[]; step: number; height: number; centered?: boolean }) {
  const every = Math.ceil(points.length / 6);
  return (
    <>
      {points.map((p, i) =>
        i % every === 0 ? (
          <text key={i} x={PAD.left + step * i + (centered ? step / 2 : 0)} y={height - 6} textAnchor="middle" fontSize={11} fill="var(--faint)">
            {p.label}
          </text>
        ) : null,
      )}
    </>
  );
}

/** A thin progress bar. */
export function Progress({ value, className }: { value: number; className?: string }) {
  return (
    <div className={`h-2 overflow-hidden rounded-full bg-surface-3 ${className ?? ""}`}>
      <div className="h-full rounded-full bg-accent transition-[width] duration-700" style={{ width: `${Math.min(100, Math.max(0, value * 100))}%` }} />
    </div>
  );
}
