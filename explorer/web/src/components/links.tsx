import { Check, Copy } from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router";
import { exactTess, formatDate, formatTess, shortAddress, shortHash, timeAgo } from "../lib/format";
import { useNow } from "../lib/hooks";
import { cx } from "./ui";

/** FNV-1a, to derive stable colours and patterns from an address. */
function hash32(text: string, seed = 0x811c9dc5): number {
  let h = seed;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** A mosaic of tesserae drawn from an address: symmetric 5×5 tiles in two blues. */
export function Identicon({ address, size = 24, className }: { address: string; size?: number; className?: string }) {
  const tiles = useMemo(() => {
    const h = hash32(address);
    const h2 = hash32(address, h);
    const hue = 200 + (h % 60);
    const out: Array<{ x: number; y: number; fill: string }> = [];
    for (let y = 0; y < 5; y++) {
      for (let x = 0; x < 3; x++) {
        const bit = (y * 3 + x) % 30;
        const on = ((bit < 15 ? h : h2) >> bit % 15) & 1;
        if (!on) continue;
        const strong = ((h2 >> (y * 3 + x)) & 1) === 1;
        const fill = strong ? `hsl(${hue} 85% 52%)` : `hsl(${(hue + 30) % 360} 70% 38%)`;
        out.push({ x, y, fill });
        if (x < 2) out.push({ x: 4 - x, y, fill });
      }
    }
    return out;
  }, [address]);
  return (
    <svg width={size} height={size} viewBox="0 0 5 5" className={cx("shrink-0 rounded-[28%] bg-surface-3", className)} aria-hidden="true">
      {tiles.map((t) => (
        <rect key={`${t.x}-${t.y}`} x={t.x + 0.08} y={t.y + 0.08} width={0.84} height={0.84} rx={0.12} fill={t.fill} />
      ))}
    </svg>
  );
}

export function CopyButton({ value, className }: { value: string; className?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      aria-label="Copy"
      title="Copy"
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        void navigator.clipboard?.writeText(value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        });
      }}
      className={cx("inline-flex size-6 shrink-0 items-center justify-center rounded-md text-faint hover:bg-surface-3 hover:text-fg", className)}
    >
      {copied ? <Check size={13} className="text-success" /> : <Copy size={13} />}
    </button>
  );
}

export function AddressLink({ address, label, full, copy, className }: { address: string | null | undefined; label?: string | null; full?: boolean; copy?: boolean; className?: string }) {
  if (!address) return <span className="text-faint">—</span>;
  return (
    <span className={cx("inline-flex min-w-0 max-w-full items-center gap-2", className)}>
      <Identicon address={address} size={20} />
      <Link to={`/account/${address}`} className="min-w-0 truncate font-mono text-[13px] text-accent hover:text-accent-strong" title={address}>
        {label ? <span className="font-sans text-sm font-medium">{label}</span> : full ? address : shortAddress(address)}
      </Link>
      {copy && <CopyButton value={address} />}
    </span>
  );
}

export function BlockLink({ number, className }: { number: number | null | undefined; className?: string }) {
  if (number == null) return <span className="text-faint">—</span>;
  return (
    <Link to={`/block/${number}`} className={cx("font-medium tabular text-accent hover:text-accent-strong", className)}>
      #{number.toLocaleString("en-US")}
    </Link>
  );
}

export function HashText({ hash, to, short = true, copy = true }: { hash: string | null | undefined; to?: string; short?: boolean; copy?: boolean }) {
  if (!hash) return <span className="text-faint">—</span>;
  const text = short ? shortHash(hash, 8, 6) : hash;
  return (
    <span className="inline-flex min-w-0 max-w-full items-center gap-1">
      {to ? (
        <Link to={to} className="min-w-0 truncate font-mono text-[13px] text-accent hover:text-accent-strong" title={hash}>
          {text}
        </Link>
      ) : (
        <span className={cx("min-w-0 font-mono text-[13px]", short ? "truncate" : "break-all")} title={hash}>
          {text}
        </span>
      )}
      {copy && <CopyButton value={hash} />}
    </span>
  );
}

export function Amount({ planck, decimals = 4, unit = true, className }: { planck: string | null | undefined; decimals?: number; unit?: boolean; className?: string }) {
  if (planck == null) return <span className="text-faint">—</span>;
  return (
    <span className={cx("whitespace-nowrap tabular", className)} title={`${exactTess(planck)} TESS`}>
      {formatTess(planck, decimals)}
      {unit && <span className="ml-1 text-[0.85em] font-medium text-muted">TESS</span>}
    </span>
  );
}

export function TimeAgo({ date, className }: { date: string | null | undefined; className?: string }) {
  const now = useNow(5000);
  if (!date) return <span className="text-faint">—</span>;
  return (
    <time dateTime={date} title={formatDate(date)} className={cx("whitespace-nowrap text-muted", className)}>
      {timeAgo(date, now)}
    </time>
  );
}
