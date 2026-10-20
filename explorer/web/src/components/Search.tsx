import { Search as SearchIcon } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { supabase } from "../lib/supabase";
import { isAddress, isBlockNumber, isHash } from "../lib/format";
import { cx } from "./ui";

/** Where a query leads: straight to a block, an account or an extrinsic, else to the results. */
export async function resolveQuery(raw: string): Promise<string> {
  const query = raw.trim();
  if (isBlockNumber(query)) return `/block/${query.replace("#", "")}`;
  if (isAddress(query)) return `/account/${query}`;
  if (/^\d+-\d+$/.test(query)) return `/extrinsic/${query}`;
  if (isHash(query)) {
    const { data } = await supabase.rpc("explorer_search", { query: query.toLowerCase() });
    const hit = (data as Array<{ kind: string; id: string }> | null)?.[0];
    if (hit?.kind === "block") return `/block/${hit.id}`;
    if (hit?.kind === "extrinsic") return `/extrinsic/${hit.id}`;
  }
  return `/search?q=${encodeURIComponent(query)}`;
}

export function SearchBar({ large, className, onDone }: { large?: boolean; className?: string; onDone?: () => void }) {
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const navigate = useNavigate();
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;
    setBusy(true);
    try {
      navigate(await resolveQuery(query));
      setQuery("");
      onDone?.();
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className={cx("relative", className)} role="search">
      <SearchIcon size={large ? 20 : 16} className={cx("pointer-events-none absolute top-1/2 -translate-y-1/2 text-faint", large ? "left-4" : "left-3")} />
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={large ? "Search by block, transaction hash, address, agent name, task or channel" : "Block, hash, address, agent…"}
        aria-label="Search"
        className={cx(
          "w-full rounded-xl border border-line bg-surface text-fg placeholder:text-faint outline-none transition-shadow focus:border-accent focus:ring-4 focus:ring-accent-soft",
          large ? "h-14 pl-12 pr-28 text-[15px] shadow-sm" : "h-10 pl-9 pr-3 text-sm",
        )}
      />
      {large && (
        <button type="submit" disabled={busy} className="absolute right-2 top-1/2 h-10 -translate-y-1/2 rounded-lg bg-accent px-4 text-sm font-semibold text-white hover:bg-accent-strong disabled:opacity-60">
          Search
        </button>
      )}
    </form>
  );
}
