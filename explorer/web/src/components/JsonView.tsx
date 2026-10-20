import { useState } from "react";
import { hexText, isAddress } from "../lib/format";
import { AddressLink, CopyButton } from "./links";

/** Decoded call arguments and event data: addresses as links, byte strings as text when they
 * are text, large blobs summarized. */
export function JsonView({ value }: { value: unknown }) {
  if (value == null || (typeof value === "object" && Object.keys(value as object).length === 0)) {
    return <span className="text-sm text-faint">No arguments</span>;
  }
  return (
    <div className="overflow-x-auto rounded-xl border border-line bg-surface-2 p-3 font-mono text-[12.5px] leading-relaxed">
      <Node value={value} depth={0} />
    </div>
  );
}

function Leaf({ value }: { value: unknown }) {
  if (value === null) return <span className="text-faint">null</span>;
  if (typeof value === "boolean") return <span className="text-warning">{String(value)}</span>;
  if (typeof value === "number") return <span className="text-success">{value}</span>;
  const text = String(value);
  if (isAddress(text)) return <AddressLink address={text} full />;
  if (/^\d+$/.test(text)) return <span className="text-success">{text}</span>;
  if (text.startsWith("0x")) {
    const decoded = hexText(text);
    return (
      <span className="inline-flex max-w-full items-center gap-1 align-middle">
        {decoded ? (
          <span className="text-fg" title={text}>
            “{decoded}”
          </span>
        ) : (
          <span className="break-all text-muted">{text.length > 132 ? `${text.slice(0, 66)}…${text.slice(-64)}` : text}</span>
        )}
        <CopyButton value={text} />
      </span>
    );
  }
  return <span className="text-fg">“{text}”</span>;
}

function Node({ value, depth }: { value: unknown; depth: number }) {
  const [open, setOpen] = useState(depth < 3);
  if (value === null || typeof value !== "object") return <Leaf value={value} />;
  const object = value as Record<string, unknown>;
  // Summarized blobs: { bytes, blake2 }.
  if (Object.keys(object).length === 2 && typeof object.bytes === "number" && typeof object.blake2 === "string") {
    return (
      <span className="text-muted">
        {object.bytes.toLocaleString("en-US")} bytes, blake2 <span className="break-all">{String(object.blake2)}</span>
      </span>
    );
  }
  const entries = Array.isArray(value) ? value.map((v, i) => [String(i), v] as const) : Object.entries(object);
  if (entries.length === 0) return <span className="text-faint">{Array.isArray(value) ? "[]" : "{}"}</span>;
  return (
    <div>
      <button className="text-faint hover:text-fg" onClick={() => setOpen(!open)}>
        {Array.isArray(value) ? `[${entries.length}]` : `{${entries.length}}`} {open ? "▾" : "▸"}
      </button>
      {open && (
        <div className="ml-3 border-l border-line pl-3">
          {entries.map(([key, inner]) => (
            <div key={key} className="flex min-w-0 flex-wrap items-baseline gap-x-2 py-0.5">
              <span className="text-accent">{key}</span>
              <span className="min-w-0 max-w-full">
                <Node value={inner} depth={depth + 1} />
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
