import { Blocks, Bot, Database, ScrollText, Search, Users, Wallet, Zap } from "lucide-react";
import type { ReactNode } from "react";
import { Link, useSearchParams } from "react-router";
import { Page } from "../components/Layout";
import { SearchBar } from "../components/Search";
import { Card, EmptyState, ErrorState, PageHeader, SkeletonRows } from "../components/ui";
import { useSearch, type SearchResult } from "../lib/api";

const icons: Record<SearchResult["kind"], ReactNode> = {
  block: <Blocks size={16} />,
  extrinsic: <ScrollText size={16} />,
  account: <Wallet size={16} />,
  agent: <Bot size={16} />,
  task: <Users size={16} />,
  channel: <Zap size={16} />,
};

const path = (r: SearchResult) => (r.kind === "extrinsic" ? `/extrinsic/${r.id}` : r.kind === "account" ? `/account/${r.id}` : `/${r.kind}/${r.id}`);

export function SearchPage() {
  const [params] = useSearchParams();
  const query = params.get("q") ?? "";
  const { data, error, isLoading } = useSearch(query);
  return (
    <Page className="space-y-6">
      <PageHeader title="Search" icon={<Search size={20} />} subtitle={query ? <>Results for “{query}”</> : undefined} />
      <SearchBar large />
      <Card>
        {error ? (
          <ErrorState error={error} />
        ) : isLoading ? (
          <SkeletonRows rows={4} />
        ) : !data?.length ? (
          <EmptyState title="Nothing found">Search for a block number or hash, a transaction hash, an address (te…), an agent name, a task or a channel id.</EmptyState>
        ) : (
          <ul className="divide-y divide-line">
            {data.map((r) => (
              <li key={`${r.kind}-${r.id}`}>
                <Link to={path(r)} className="flex items-center gap-3 px-5 py-3.5 hover:bg-surface-2">
                  <span className="flex size-9 items-center justify-center rounded-xl bg-accent-soft text-accent">{icons[r.kind]}</span>
                  <span className="min-w-0">
                    <span className="block truncate font-medium">{r.label}</span>
                    <span className="block text-xs capitalize text-muted">{r.kind}</span>
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </Page>
  );
}

export function NotFound() {
  return (
    <Page>
      <Card>
        <EmptyState title="Page not found">
          <Link to="/" className="text-accent">Back to the explorer</Link>
        </EmptyState>
      </Card>
    </Page>
  );
}

/** Shown when the build has no Supabase project. */
export function NotConfigured() {
  return (
    <div className="flex min-h-dvh items-center justify-center p-6">
      <div className="max-w-lg rounded-2xl border border-line bg-surface p-8 text-center">
        <img src="/logo.png" alt="" className="mx-auto mb-4 size-16" />
        <h1 className="text-xl font-semibold">Tesserax Explorer is not configured</h1>
        <p className="mt-2 text-sm text-muted">
          Set <code className="font-mono">VITE_SUPABASE_URL</code> and <code className="font-mono">VITE_SUPABASE_ANON_KEY</code> to the Supabase project the indexer writes to, in <code className="font-mono">explorer/web/.env</code>.
        </p>
        <Database className="mx-auto mt-5 text-faint" size={22} />
      </div>
    </div>
  );
}
