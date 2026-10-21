import { ChevronLeft, ChevronRight, Inbox, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Link, useSearchParams } from "react-router";

export const cx = (...classes: Array<string | false | null | undefined>) => classes.filter(Boolean).join(" ");

export function Card({ children, className }: { children: ReactNode; className?: string }) {
  // min-w-0: in a grid, a card shrinks below the width of long hashes and addresses.
  return <section className={cx("min-w-0 rounded-2xl border border-line bg-surface shadow-[0_1px_0_rgba(0,0,0,0.02)]", className)}>{children}</section>;
}

export function CardHeader({ title, icon, action, children }: { title: ReactNode; icon?: ReactNode; action?: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line px-4 py-3.5 sm:px-5">
      <div className="flex min-w-0 items-center gap-2.5">
        {icon && <span className="text-accent">{icon}</span>}
        <h2 className="truncate text-[15px] font-semibold tracking-tight">{title}</h2>
        {children}
      </div>
      {action}
    </div>
  );
}

export function CardLink({ to, children }: { to: string; children: ReactNode }) {
  return (
    <Link to={to} className="inline-flex items-center gap-1 text-sm font-medium text-accent hover:text-accent-strong">
      {children}
      <ChevronRight size={15} />
    </Link>
  );
}

export function PageHeader({ title, subtitle, icon, children }: { title: ReactNode; subtitle?: ReactNode; icon?: ReactNode; children?: ReactNode }) {
  return (
    <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div className="flex min-w-0 items-start gap-3">
        {icon && <div className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-xl border border-line bg-surface text-accent">{icon}</div>}
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold tracking-tight sm:text-[28px]">{title}</h1>
          {subtitle && <div className="mt-1 text-sm text-muted">{subtitle}</div>}
        </div>
      </div>
      {children}
    </div>
  );
}

type Tone = "neutral" | "accent" | "success" | "danger" | "warning";

const tones: Record<Tone, string> = {
  neutral: "bg-surface-3 text-muted border-line",
  accent: "bg-accent-soft text-accent border-transparent",
  success: "bg-success-soft text-success border-transparent",
  danger: "bg-danger-soft text-danger border-transparent",
  warning: "bg-warning-soft text-warning border-transparent",
};

export function Badge({ tone = "neutral", children, className }: { tone?: Tone; children: ReactNode; className?: string }) {
  return (
    <span className={cx("inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", tones[tone], className)}>
      {children}
    </span>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <span className={cx("block animate-pulse rounded-md bg-surface-3", className)} />;
}

export function SkeletonRows({ rows = 6 }: { rows?: number }) {
  return (
    <div className="divide-y divide-line">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-4 px-5 py-4">
          <Skeleton className="size-9 rounded-xl" />
          <div className="flex-1 space-y-2">
            <Skeleton className="h-3.5 w-1/3" />
            <Skeleton className="h-3 w-1/2" />
          </div>
          <Skeleton className="h-3.5 w-16" />
        </div>
      ))}
    </div>
  );
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-14 text-center">
      <Inbox className="text-faint" size={28} />
      <p className="font-medium">{title}</p>
      {children && <p className="max-w-md text-sm text-muted">{children}</p>}
    </div>
  );
}

export function ErrorState({ error }: { error: unknown }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 px-6 py-14 text-center">
      <TriangleAlert className="text-danger" size={28} />
      <p className="font-medium">Something went wrong</p>
      <p className="max-w-md text-sm text-muted">{error instanceof Error ? error.message : String(error)}</p>
    </div>
  );
}

/** Rows of label and value, for detail pages. */
export function Fields({ children, narrow }: { children: ReactNode; narrow?: boolean }) {
  // In a side column, labels take less room than in a full-width card.
  return <dl className={cx("divide-y divide-line", narrow && "sm:[&>div]:grid-cols-[130px_minmax(0,1fr)]")}>{children}</dl>;
}

export function Field({ label, children, hint }: { label: ReactNode; children: ReactNode; hint?: string }) {
  return (
    <div className="grid gap-1 px-4 py-3.5 sm:grid-cols-[220px_1fr] sm:gap-4 sm:px-5">
      <dt className="text-sm text-muted" title={hint}>
        {label}
      </dt>
      <dd className="min-w-0 break-words text-sm">{children}</dd>
    </div>
  );
}

export function StatCard({ label, value, sub, icon, loading }: { label: string; value: ReactNode; sub?: ReactNode; icon?: ReactNode; loading?: boolean }) {
  return (
    <div className="rounded-2xl border border-line bg-surface p-4 sm:p-5">
      <div className="flex items-center justify-between gap-2 text-sm text-muted">
        <span className="truncate">{label}</span>
        {icon && <span className="text-faint">{icon}</span>}
      </div>
      <div className="mt-2 truncate text-xl font-semibold tracking-tight tabular sm:text-2xl">{loading ? <Skeleton className="h-7 w-24" /> : value}</div>
      {sub && <div className="mt-1 truncate text-xs text-muted">{sub}</div>}
    </div>
  );
}

/** Previous and next pages through the `page` search parameter. */
export function Pagination({ index, more }: { index: number; more: boolean }) {
  const [params, setParams] = useSearchParams();
  const go = (page: number) => {
    const next = new URLSearchParams(params);
    if (page <= 1) next.delete("page");
    else next.set("page", String(page));
    setParams(next);
    window.scrollTo({ top: 0, behavior: "smooth" });
  };
  if (index === 0 && !more) return null;
  const button = "inline-flex h-9 items-center gap-1 rounded-lg border border-line bg-surface px-3 text-sm font-medium hover:border-line-strong disabled:pointer-events-none disabled:opacity-40";
  return (
    <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3 sm:px-5">
      <button className={button} disabled={index === 0} onClick={() => go(index)}>
        <ChevronLeft size={16} /> Newer
      </button>
      <span className="text-sm text-muted tabular">Page {index + 1}</span>
      <button className={button} disabled={!more} onClick={() => go(index + 2)}>
        Older <ChevronRight size={16} />
      </button>
    </div>
  );
}

export function Tabs<T extends string>({ tabs, value, onChange }: { tabs: Array<{ id: T; label: ReactNode; count?: number }>; value: T; onChange: (id: T) => void }) {
  return (
    <div className="scrollbar-none -mb-px flex gap-1 overflow-x-auto px-3 sm:px-4" role="tablist">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          aria-selected={value === tab.id}
          onClick={() => onChange(tab.id)}
          className={cx(
            "flex shrink-0 items-center gap-2 border-b-2 px-2.5 py-3 text-sm font-medium transition-colors",
            value === tab.id ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg",
          )}
        >
          {tab.label}
          {tab.count != null && <span className="rounded-full bg-surface-3 px-1.5 text-xs tabular text-muted">{tab.count}</span>}
        </button>
      ))}
    </div>
  );
}

/** Pills to filter a list, in the URL. */
export function Filters<T extends string>({ name, options, value }: { name: string; options: Array<{ id: T | ""; label: string }>; value: T | "" }) {
  const [params, setParams] = useSearchParams();
  return (
    <div className="scrollbar-none flex gap-1.5 overflow-x-auto">
      {options.map((option) => (
        <button
          key={option.id}
          onClick={() => {
            const next = new URLSearchParams(params);
            next.delete("page");
            if (option.id) next.set(name, option.id);
            else next.delete(name);
            setParams(next);
          }}
          className={cx(
            "h-8 shrink-0 rounded-full border px-3 text-sm font-medium transition-colors",
            value === option.id ? "border-accent bg-accent-soft text-accent" : "border-line bg-surface text-muted hover:text-fg",
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
