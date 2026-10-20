import { Activity, Blocks, Bot, Clock, Coins, Gauge, Pickaxe, ScrollText, Zap } from "lucide-react";
import { useRef } from "react";
import { Link } from "react-router";
import { BarChart, LineChart, Progress } from "../components/charts";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, HashText, Identicon, TimeAgo } from "../components/links";
import { SearchBar } from "../components/Search";
import { signedCount } from "./Blocks";
import { Badge, Card, CardHeader, CardLink, ErrorState, SkeletonRows, StatCard } from "../components/ui";
import { useChainState, useDailyStats, useLatestBlocks, useLatestExtrinsics, useOverview, type Block } from "../lib/api";
import { callLabel, formatCompact, formatDuration, formatHashrate, formatNumber, formatTess, shortAddress, TARGET_BLOCK_SECONDS, tessNumber } from "../lib/format";

function Hero() {
  return (
    <div className="relative overflow-hidden border-b border-line">
      <div className="mosaic absolute inset-0" />
      <div className="glow absolute inset-0" />
      <div className="relative mx-auto max-w-4xl px-4 pb-12 pt-12 text-center sm:px-6 sm:pb-16 sm:pt-16">
        <img src="/logo.png" alt="Tesserax" className="mx-auto mb-5 size-20 drop-shadow-[0_8px_30px_rgba(77,134,255,0.35)] sm:size-24" />
        <h1 className="text-3xl font-bold tracking-tight sm:text-5xl">The Tesserax explorer</h1>
        <p className="mx-auto mt-3 max-w-2xl text-[15px] text-muted sm:text-lg">
          Blocks, transactions, miners and AI agents of the post-quantum proof-of-work chain built for agents.
        </p>
        <SearchBar large className="mx-auto mt-7 max-w-3xl text-left" />
      </div>
    </div>
  );
}

function Stats() {
  const overview = useOverview();
  const state = useChainState();
  const o = overview.data;
  const supply = state.data?.supply;
  const loading = overview.isLoading;
  const minedShare = supply ? tessNumber(supply.mined) / Math.max(1, tessNumber(supply.mining_supply)) : 0;
  return (
    <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
      <StatCard label="Block height" icon={<Blocks size={16} />} loading={loading} value={o?.best != null ? `#${formatNumber(o.best)}` : "—"} sub={o?.finalized != null ? `Final up to #${formatNumber(o.finalized)}` : undefined} />
      <StatCard label="Hashrate" icon={<Pickaxe size={16} />} loading={loading} value={formatHashrate(o?.hashrate)} sub={`${formatNumber(o?.miners_24h ?? 0)} miners in 24 h`} />
      <StatCard label="Difficulty" icon={<Gauge size={16} />} loading={loading} value={formatCompact(o?.difficulty)} sub="LWMA over 120 blocks" />
      <StatCard label="Block time" icon={<Clock size={16} />} loading={loading} value={formatDuration(o?.block_time)} sub={`Target ${TARGET_BLOCK_SECONDS} s`} />
      <StatCard
        label="Mined supply"
        icon={<Coins size={16} />}
        loading={state.isLoading}
        value={supply ? `${formatCompact(tessNumber(supply.mined))} TESS` : "—"}
        sub={supply ? <Progress value={minedShare} className="mt-1.5" /> : undefined}
      />
      <StatCard label="Block reward" icon={<Zap size={16} />} loading={state.isLoading} value={supply ? `${formatTess(supply.block_reward)} TESS` : "—"} sub={supply ? `Halving at #${formatNumber(supply.next_halving)}` : undefined} />
      <StatCard label="Transactions, 24 h" icon={<ScrollText size={16} />} loading={loading} value={formatNumber(o?.extrinsics_24h ?? 0)} sub={o ? `${formatNumber(o.transfers_24h)} transfers · ${formatTess(o.volume_24h, 0)} TESS` : undefined} />
      <StatCard label="AI agents" icon={<Bot size={16} />} loading={loading} value={formatNumber(o?.agents ?? 0)} sub={o ? `${formatNumber(o.open_tasks)} open tasks · ${formatNumber(o.open_channels)} channels` : undefined} />
    </div>
  );
}

function Charts() {
  const { data } = useDailyStats(30);
  const days = (data ?? []).map((d) => ({ ...d, label: new Date(d.day).toLocaleDateString(undefined, { month: "short", day: "numeric" }) }));
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Card>
        <CardHeader title="Transactions per day" icon={<Activity size={16} />} />
        <div className="p-3 sm:p-4">
          <BarChart points={days.map((d) => ({ label: d.label, value: Number(d.extrinsics), display: `${formatNumber(d.extrinsics)} transactions, ${formatNumber(d.transfers)} transfers` }))} />
        </div>
      </Card>
      <Card>
        <CardHeader title="Difficulty" icon={<Gauge size={16} />} />
        <div className="p-3 sm:p-4">
          <LineChart points={days.map((d) => ({ label: d.label, value: Number(d.avg_difficulty ?? 0), display: `${formatCompact(d.avg_difficulty)} · ${formatDuration(d.avg_block_time)} per block` }))} />
        </div>
      </Card>
    </div>
  );
}

function LatestBlocks() {
  const { data, error, isLoading } = useLatestBlocks(8);
  const seen = useRef<number | null>(null);
  const previous = seen.current;
  if (data?.[0]) seen.current = data[0].number;
  const fresh = (b: Block) => previous != null && b.number > previous;
  return (
    <Card>
      <CardHeader title="Latest blocks" icon={<Blocks size={16} />} action={<CardLink to="/blocks">All blocks</CardLink>} />
      {error ? (
        <ErrorState error={error} />
      ) : isLoading ? (
        <SkeletonRows rows={8} />
      ) : (
        <ul className="divide-y divide-line">
          {data?.map((b) => (
            <li key={b.hash} className={`flex items-center gap-3 px-4 py-3 sm:px-5 ${fresh(b) ? "fresh" : ""}`}>
              <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
                <Blocks size={18} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <BlockLink number={b.number} />
                  {b.finalized && <Badge tone="success">Final</Badge>}
                </div>
                <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted">
                  <span>Miner</span>
                  {b.author ? (
                    <Link to={`/account/${b.author}`} className="inline-flex min-w-0 items-center gap-1 font-mono text-accent">
                      <Identicon address={b.author} size={14} />
                      <span className="truncate">{shortAddress(b.author, 5, 4)}</span>
                    </Link>
                  ) : (
                    "—"
                  )}
                </div>
              </div>
              <div className="shrink-0 text-right">
                <div className="text-sm">
                  <Amount planck={(BigInt(b.reward) + BigInt(b.fees)).toString()} decimals={3} />
                </div>
                <div className="mt-0.5 text-xs text-muted">
                  {signedCount(b)} tx · <TimeAgo date={b.timestamp} />
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

function LatestExtrinsics() {
  const { data, error, isLoading } = useLatestExtrinsics(8);
  return (
    <Card>
      <CardHeader title="Latest transactions" icon={<ScrollText size={16} />} action={<CardLink to="/extrinsics">All transactions</CardLink>} />
      {error ? (
        <ErrorState error={error} />
      ) : isLoading ? (
        <SkeletonRows rows={8} />
      ) : data?.length === 0 ? (
        <div className="px-5 py-14 text-center text-sm text-muted">No signed transaction yet.</div>
      ) : (
        <ul className="divide-y divide-line">
          {data?.map((x) => (
            <li key={`${x.block_hash}-${x.index}`} className="flex items-center gap-3 px-4 py-3 sm:px-5">
              <div className={`flex size-10 shrink-0 items-center justify-center rounded-xl ${x.success ? "bg-surface-3 text-muted" : "bg-danger-soft text-danger"}`}>
                <ScrollText size={18} />
              </div>
              <div className="min-w-0 flex-1">
                <HashText hash={x.hash} to={`/extrinsic/${x.hash}`} copy={false} />
                <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted">
                  <span className="truncate">{callLabel(x.pallet, x.call)}</span>
                </div>
              </div>
              <div className="shrink-0 text-right">
                <AddressLink address={x.signer} className="justify-end" />
                <div className="mt-0.5 text-xs text-muted">
                  <TimeAgo date={x.timestamp} />
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

export function Home() {
  return (
    <>
      <Hero />
      <Page className="space-y-6">
        <Stats />
        <div className="grid gap-4 lg:grid-cols-2">
          <LatestBlocks />
          <LatestExtrinsics />
        </div>
        <Charts />
      </Page>
    </>
  );
}
