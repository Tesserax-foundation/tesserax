import { Coins, Cpu, Lock, Pickaxe, Shield } from "lucide-react";
import { Link, useSearchParams } from "react-router";
import { Progress } from "../components/charts";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, TimeAgo } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, Field, Fields, PageHeader, Pagination, StatCard } from "../components/ui";
import { useChainState, useShieldedActivity } from "../lib/api";
import { blocksDuration, formatCompact, formatNumber, formatTess, tessNumber } from "../lib/format";
import { pageIndex } from "../lib/hooks";

const TOTAL_SUPPLY = 14_000_000;

/** The reward of each halving era, while it is above zero. */
function eras(initial: bigint, interval: number) {
  const rows: Array<{ era: number; from: number; to: number; reward: bigint; minted: bigint }> = [];
  for (let era = 0; era < 12; era++) {
    const reward = initial >> BigInt(era);
    if (reward === 0n) break;
    rows.push({ era, from: era * interval + 1, to: (era + 1) * interval, reward, minted: reward * BigInt(interval) });
  }
  return rows;
}

export function SupplyPage() {
  const { data, isLoading } = useChainState();
  const supply = data?.supply;
  const constants = data?.constants;
  const best = data?.head?.best ?? 0;
  const issuance = tessNumber(supply?.total_issuance);
  const mined = tessNumber(supply?.mined);
  const miningSupply = tessNumber(supply?.mining_supply ?? constants?.mining_supply);
  const halving = constants?.halving_interval ?? 7_000_000;
  const schedule = constants ? eras(BigInt(constants.initial_reward), halving) : [];
  const blocksToHalving = supply ? supply.next_halving - (best + 1) : null;
  return (
    <Page className="space-y-6">
      <PageHeader title="Supply and emission" icon={<Cpu size={20} />} subtitle="14 million TESS: 85% mined, 10% to the team over four years, 5% to the foundation. Nothing is burned: miners receive every reward and fee." />

      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
        <StatCard label="Total issuance" icon={<Coins size={16} />} loading={isLoading} value={`${formatCompact(issuance)} TESS`} sub={`${((issuance / TOTAL_SUPPLY) * 100).toFixed(2)}% of 14 M`} />
        <StatCard label="Mined" icon={<Pickaxe size={16} />} loading={isLoading} value={`${formatCompact(mined)} TESS`} sub={<Progress value={miningSupply ? mined / miningSupply : 0} className="mt-1.5" />} />
        <StatCard label="Block reward" loading={isLoading} value={supply ? `${formatTess(supply.block_reward)} TESS` : "—"} sub={`Era ${supply?.halving_era ?? 0}`} />
        <StatCard label="Next halving" loading={isLoading} value={supply ? `#${formatNumber(supply.next_halving)}` : "—"} sub={blocksToHalving != null ? `in about ${blocksDuration(blocksToHalving)}` : undefined} />
      </div>

      <div className="grid gap-6 lg:grid-cols-2">
        <Card>
          <CardHeader title="Distribution" />
          <div className="space-y-5 p-5">
            {[
              { label: "Mining", share: 0.85, amount: "11,900,000", note: "Block rewards, halving every 7 000 000 blocks (about 4 years)" },
              { label: "Team", share: 0.1, amount: "1,400,000", note: "48 months of vesting, with a 6-month cliff" },
              { label: "Foundation", share: 0.05, amount: "700,000", note: "140 000 liquid at genesis, the rest over 36 months" },
            ].map((part) => (
              <div key={part.label}>
                <div className="mb-1.5 flex items-baseline justify-between gap-2">
                  <span className="font-medium">{part.label}</span>
                  <span className="text-sm tabular text-muted">{part.amount} TESS · {part.share * 100}%</span>
                </div>
                <Progress value={part.share} />
                <p className="mt-1 text-xs text-muted">{part.note}</p>
              </div>
            ))}
          </div>
        </Card>
        <Card>
          <CardHeader title="Emission schedule" />
          <Table
            rows={schedule}
            rowKey={(r) => r.era}
            columns={[
              { header: "Era", cell: (r) => <span className="inline-flex items-center gap-2 tabular">{r.era}{supply?.halving_era === r.era && <Badge tone="accent">Now</Badge>}</span> },
              { header: "Blocks", cell: (r) => <span className="tabular text-sm">{formatCompact(r.from)} – {formatCompact(r.to)}</span> },
              { header: "Reward", align: "right", cell: (r) => <Amount planck={r.reward.toString()} decimals={6} /> },
              { header: "Minted", align: "right", desktopOnly: true, cell: (r) => <Amount planck={r.minted.toString()} decimals={0} /> },
            ]}
          />
        </Card>
      </div>

      <Card>
        <CardHeader title="Genesis allocations" icon={<Lock size={16} />} />
        <Table
          rows={data?.allocations}
          loading={isLoading}
          rowKey={(a) => a.account}
          empty="No genesis allocation on this network"
          columns={[
            { header: "Account", cell: (a) => <AddressLink address={a.account} /> },
            { header: "Allocation", align: "right", cell: (a) => <Amount planck={a.amount} decimals={0} /> },
            { header: "Liquid at genesis", align: "right", desktopOnly: true, cell: (a) => <Amount planck={a.liquid} decimals={0} /> },
            { header: "Cliff", align: "right", desktopOnly: true, cell: (a) => (a.cliff ? blocksDuration(a.cliff) : "None") },
            { header: "Vesting", align: "right", cell: (a) => blocksDuration(a.duration) },
          ]}
        />
      </Card>

      {constants && (
        <Card>
          <CardHeader title="Parameters" />
          <Fields>
            <Field label="Token">TESS, {constants.decimals} decimals</Field>
            <Field label="Target block time">{constants.target_block_time_ms / 1000} s, difficulty from the last {constants.difficulty_window} blocks</Field>
            <Field label="Initial reward"><Amount planck={constants.initial_reward} decimals={6} /></Field>
            <Field label="Existential deposit"><Amount planck={constants.existential_deposit} decimals={6} /></Field>
            <Field label="Address prefix">SS58 {constants.ss58_prefix}, addresses start with “te”</Field>
            <Field label="Signatures">ML-DSA-87 (FIPS 204), post-quantum</Field>
            <Field label="Runtime">Spec version {constants.spec_version}</Field>
          </Fields>
        </Card>
      )}
    </Page>
  );
}

export function ShieldedPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const state = useChainState();
  const supply = state.data?.supply;
  const { data, isLoading, error } = useShieldedActivity(index);
  return (
    <Page className="space-y-6">
      <PageHeader title="Shielded pool" icon={<Shield size={20} />} subtitle="Private transfers proven with zk-STARKs: amounts, senders and recipients stay hidden. Only the pool total and the value entering or leaving it are public." />
      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-3">
        <StatCard label="Pool balance" loading={state.isLoading} value={supply ? <Amount planck={supply.shielded_pool} decimals={2} /> : "—"} />
        <StatCard label="Notes" loading={state.isLoading} value={formatNumber(supply?.shielded_notes ?? 0)} sub="Commitments in the note tree" />
        <StatCard label="Minimum fee" loading={state.isLoading} value={state.data?.constants ? <Amount planck={state.data.constants.shielded_minimum_fee} decimals={6} /> : "—"} />
      </div>
      <Card>
        <CardHeader title="Shielded transactions" />
        <Table
          rows={data?.rows}
          loading={isLoading}
          error={error}
          rowKey={(e) => `${e.block_hash}-${e.index}`}
          empty="No shielded transaction yet"
          columns={[
            { header: "Transaction", cell: (e) => (e.extrinsic_index != null ? <Link to={`/extrinsic/${e.block_number}-${e.extrinsic_index}`} className="tabular text-accent">{e.block_number}-{e.extrinsic_index}</Link> : <BlockLink number={e.block_number} />) },
            { header: "Age", cell: (e) => <TimeAgo date={e.timestamp} /> },
            { header: "Notes", align: "right", cell: (e) => <span className="tabular">{(e.data.nullifiers as unknown[] | undefined)?.length ?? 0} spent · {(e.data.commitments as unknown[] | undefined)?.length ?? 0} created</span> },
            { header: "In", align: "right", cell: (e) => <Amount planck={String(e.data.value_in ?? "0")} decimals={4} /> },
            { header: "Out", align: "right", cell: (e) => <Amount planck={String(e.data.value_out ?? "0")} decimals={4} /> },
            { header: "Fee", align: "right", desktopOnly: true, cell: (e) => <Amount planck={String(e.data.fee ?? "0")} decimals={6} /> },
          ]}
        />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}
