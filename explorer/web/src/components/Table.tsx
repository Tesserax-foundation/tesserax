import type { ReactNode } from "react";
import { cx, EmptyState, ErrorState, SkeletonRows } from "./ui";

export interface Column<T> {
  header: ReactNode;
  cell: (row: T) => ReactNode;
  /** Right-aligned (amounts, counts). */
  align?: "right";
  className?: string;
  /** Hidden on the stacked mobile layout. */
  desktopOnly?: boolean;
}

/** A table on wide screens, stacked cards on narrow ones. */
export function Table<T>({
  columns,
  rows,
  rowKey,
  loading,
  error,
  empty = "Nothing here yet",
  emptyHint,
  highlight,
}: {
  columns: Column<T>[];
  rows: T[] | undefined;
  rowKey: (row: T) => string | number;
  loading?: boolean;
  error?: unknown;
  empty?: string;
  emptyHint?: ReactNode;
  highlight?: (row: T) => boolean;
}) {
  if (error) return <ErrorState error={error} />;
  if (loading && !rows) return <SkeletonRows />;
  if (!rows?.length) return <EmptyState title={empty}>{emptyHint}</EmptyState>;
  return (
    <>
      <div className="hidden overflow-x-auto md:block">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-line text-left text-xs uppercase tracking-wide text-faint">
              {columns.map((c, i) => (
                <th key={i} className={cx("whitespace-nowrap px-5 py-3 font-medium", c.align === "right" && "text-right", c.className)}>
                  {c.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {rows.map((row) => (
              <tr key={rowKey(row)} className={cx("transition-colors hover:bg-surface-2", highlight?.(row) && "fresh")}>
                {columns.map((c, i) => (
                  <td key={i} className={cx("max-w-[280px] px-5 py-3.5 align-middle", c.align === "right" && "text-right", c.className)}>
                    {c.cell(row)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <ul className="divide-y divide-line md:hidden">
        {rows.map((row) => (
          <li key={rowKey(row)} className={cx("space-y-2 px-4 py-3.5", highlight?.(row) && "fresh")}>
            {columns
              .filter((c) => !c.desktopOnly)
              .map((c, i) => (
                <div key={i} className="flex min-w-0 items-center justify-between gap-3 text-sm">
                  <span className="shrink-0 text-xs uppercase tracking-wide text-faint">{c.header}</span>
                  <span className="min-w-0 truncate text-right">{c.cell(row)}</span>
                </div>
              ))}
          </li>
        ))}
      </ul>
    </>
  );
}
