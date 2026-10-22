import { Bot, Blocks, Coins, KeyRound, Lock, Pickaxe, ScrollText, Users, Wallet, Zap } from "lucide-react";
import { Link, useParams, useSearchParams } from "react-router";
import { Progress } from "../components/charts";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, CopyButton, Identicon, TimeAgo } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, CardLink, EmptyState, ErrorState, Field, Fields, Filters, PageHeader, Pagination, StatCard, Tabs } from "../components/ui";
import { useAccount, useBlocks, useChainState, useChannels, useExtrinsics, useOwnedAgents, useRichList, useTasks, useTopMiners, useTransfers, type Delegation } from "../lib/api";
import { blocksDuration, formatCompact, formatNumber, formatTess, isAddress, plural, shortAddress } from "../lib/format";
import { pageIndex } from "../lib/hooks";
import { AgentsTable } from "./Agents";
import { ChannelsTable } from "./Channels";
import { ExtrinsicsTable, signedCount } from "./Blocks";
import { TransfersTable } from "./Extrinsics";
import { TasksTable } from "./Tasks";

type Tab = "transactions" | "transfers" | "mined" | "delegations" | "agents" | "tasks" | "channels";

function DelegationsTable({ rows, side }: { rows: Delegation[]; side: "owner" | "agent" }) {
  return (
    <Table
      rows={rows}
      rowKey={(d) => `${d.owner}/${d.agent}`}
      empty="No delegation"
      columns={[
        { header: side === "owner" ? "Agent address" : "Owner", cell: (d) => <AddressLink address={side === "owner" ? d.agent : d.owner} /> },
        { header: "Budget", align: "right", cell: (d) => <span><Amount planck={d.budget} decimals={2} /> <span className="text-xs text-muted">/ {blocksDuration(d.period)}</span></span> },
        { header: "Spent", align: "right", cell: (d) => <Amount planck={d.spent} decimals={4} /> },
        { header: "Per call", align: "right", desktopOnly: true, cell: (d) => <Amount planck={d.max_per_call} decimals={2} /> },
        { header: "Allowed", desktopOnly: true, cell: (d) => <span className="flex gap-1">{d.transfers && <Badge>Transfers</Badge>}{d.contracts && <Badge>Contracts</Badge>}{d.targets.length > 0 && <Badge>{d.targets.length} targets</Badge>}</span> },
        { header: "Status", align: "right", cell: (d) => (d.active ? <Badge tone="success">Active</Badge> : <Badge>Revoked</Badge>) },
      ]}
    />
  );
}

function Vesting({ address }: { address: string }) {
  const { data } = useChainState();
  const allocation = data?.allocations?.find((a) => a.account === address);
  const best = data?.head?.best ?? 0;
  if (!allocation) return null;
  const locked = allocation.vesting.reduce((sum, s) => {
    const vested = BigInt(Math.max(0, best - s.starting_block)) * BigInt(s.per_block);
    const left = BigInt(s.locked) - vested;
    return sum + (left > 0n ? left : 0n);
  }, 0n);
  const total = BigInt(allocation.amount);
  const unlocked = total - locked;
  return (
    <Card>
      <CardHeader title="Genesis allocation" icon={<Lock size={16} />} />
      <Fields>
        <Field label="Allocation"><Amount planck={allocation.amount} decimals={0} /></Field>
        <Field label="Liquid at genesis"><Amount planck={allocation.liquid} decimals={0} /></Field>
        <Field label="Vesting">{allocation.cliff > 0 ? `${blocksDuration(allocation.cliff)} cliff, ` : ""}{blocksDuration(allocation.duration)} in total</Field>
        <Field label="Unlocked">
          <div className="max-w-md space-y-1.5">
            <div className="flex justify-between text-sm"><Amount planck={unlocked.toString()} decimals={0} /><span className="text-muted">{total > 0n ? ((Number(unlocked * 10000n / total) / 100).toFixed(2)) : 0}%</span></div>
            <Progress value={total > 0n ? Number(unlocked * 10000n / total) / 10000 : 0} />
          </div>
        </Field>
      </Fields>
    </Card>
  );
}

export function AccountPage() {
  const { address = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const tab = (params.get("tab") as Tab) ?? "transactions";
  const index = pageIndex(params);
  const { data, error, isLoading } = useAccount(address);
  const setTab = (next: Tab) => setParams(next === "transactions" ? {} : { tab: next });

  const extrinsics = useExtrinsics(index, { signer: address });
  const transfers = useTransfers(index, address);
  const mined = useBlocks(index, { author: address });
  const owned = useOwnedAgents(address);
  const tasks = useTasks(index, { account: address });
  const channels = useChannels(index, { account: address });

  if (!isAddress(address)) return <Page><Card><EmptyState title="Not a Tesserax address">Tesserax addresses start with “te”.</EmptyState></Card></Page>;
  if (error) return <Page><Card><ErrorState error={error} /></Card></Page>;

  const account = data?.account;
  const total = account ? BigInt(account.free) + BigInt(account.reserved) : 0n;
  const transferable = account ? BigInt(account.free) - BigInt(account.frozen) : 0n;
  const delegations = [...(data?.delegated ?? []), ...(data?.delegations ?? [])];

  return (
    <Page className="space-y-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
        <Identicon address={address} size={56} className="rounded-2xl" />
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-2xl font-semibold tracking-tight">{data?.agent?.name || "Account"}</h1>
            {data?.agent && !data.agent.deregistered && <Badge tone="accent"><Bot size={12} /> AI agent</Badge>}
            {(data?.minedCount ?? 0) > 0 && <Badge><Pickaxe size={12} /> Miner</Badge>}
          </div>
          <div className="mt-1 flex min-w-0 items-center gap-1">
            <span className="break-all font-mono text-sm text-muted">{address}</span>
            <CopyButton value={address} />
          </div>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
        <StatCard label="Total balance" icon={<Wallet size={16} />} loading={isLoading} value={<>{formatTess(total.toString(), 4)} <span className="text-sm font-medium text-muted">TESS</span></>} />
        <StatCard label="Transferable" icon={<Coins size={16} />} loading={isLoading} value={formatTess((transferable > 0n ? transferable : 0n).toString(), 4)} sub={account && account.frozen !== "0" ? <>Frozen <Amount planck={account.frozen} decimals={2} /></> : undefined} />
        <StatCard label="Reserved" icon={<Lock size={16} />} loading={isLoading} value={formatTess(account?.reserved ?? "0", 4)} sub="Deposits of agents, channels and tasks" />
        <StatCard label="Activity" icon={<KeyRound size={16} />} loading={isLoading} value={formatNumber(data?.extrinsicCount ?? 0)} sub={`Nonce ${account?.nonce ?? 0} · ${plural(data?.minedCount, "block")} mined`} />
      </div>

      {!isLoading && !account && (
        <Card><EmptyState title="No activity yet">This address has not received or sent anything on chain.</EmptyState></Card>
      )}

      <Vesting address={address} />

      {data?.agent && !data.agent.deregistered && (
        <Card>
          <CardHeader title="Agent profile" icon={<Bot size={16} />} action={<CardLink to={`/agent/${address}`}>Agent page</CardLink>} />
          <Fields>
            <Field label="Name">{data.agent.name}</Field>
            {data.agent.description && <Field label="Description">{data.agent.description}</Field>}
            <Field label="Capabilities"><span className="flex flex-wrap gap-1">{data.agent.capabilities.map((c) => <Badge key={c} tone="accent">{c}</Badge>)}</span></Field>
            <Field label="Reputation">{plural(data.agent.payments, "payment")} from {plural(data.agent.payers, "payer")} · {formatNumber(data.agent.positive)} positive, {formatNumber(data.agent.negative)} negative</Field>
          </Fields>
        </Card>
      )}

      <Card>
        <div className="border-b border-line">
          <Tabs
            value={tab}
            onChange={setTab}
            tabs={[
              { id: "transactions", label: <><ScrollText size={15} /> Transactions</>, count: data?.extrinsicCount },
              { id: "transfers", label: <><Coins size={15} /> Transfers</> },
              { id: "mined", label: <><Blocks size={15} /> Mined blocks</>, count: data?.minedCount },
              { id: "delegations", label: <><KeyRound size={15} /> Delegations</>, count: delegations.length },
              { id: "agents", label: <><Bot size={15} /> Agents owned</>, count: owned.data?.length },
              { id: "tasks", label: <><Users size={15} /> Tasks</> },
              { id: "channels", label: <><Zap size={15} /> Channels</> },
            ]}
          />
        </div>
        {tab === "transactions" && <><ExtrinsicsTable rows={extrinsics.data?.rows} loading={extrinsics.isLoading} error={extrinsics.error} />{extrinsics.data && <Pagination index={index} more={extrinsics.data.more} />}</>}
        {tab === "transfers" && <><TransfersTable rows={transfers.data?.rows} loading={transfers.isLoading} error={transfers.error} address={address} />{transfers.data && <Pagination index={index} more={transfers.data.more} />}</>}
        {tab === "mined" && (
          <>
            <Table
              rows={mined.data?.rows}
              loading={mined.isLoading}
              error={mined.error}
              rowKey={(b) => b.hash}
              empty="No block mined"
              columns={[
                { header: "Block", cell: (b) => <BlockLink number={b.number} /> },
                { header: "Age", cell: (b) => <TimeAgo date={b.timestamp} /> },
                { header: "Txs", align: "right", cell: (b) => formatNumber(signedCount(b)) },
                { header: "Difficulty", align: "right", desktopOnly: true, cell: (b) => formatCompact(b.difficulty) },
                { header: "Reward and fees", align: "right", cell: (b) => <Amount planck={(BigInt(b.reward) + BigInt(b.fees)).toString()} decimals={4} /> },
              ]}
            />
            {mined.data && <Pagination index={index} more={mined.data.more} />}
          </>
        )}
        {tab === "delegations" && (
          <div className="divide-y divide-line">
            {(data?.delegated.length ?? 0) > 0 && <div><div className="px-5 pt-4 text-sm font-medium">Granted to agents</div><DelegationsTable rows={data!.delegated} side="owner" /></div>}
            {(data?.delegations.length ?? 0) > 0 && <div><div className="px-5 pt-4 text-sm font-medium">Received from owners</div><DelegationsTable rows={data!.delegations} side="agent" /></div>}
            {delegations.length === 0 && <EmptyState title="No delegation">Owners give agent keys a budget with limits; the chain enforces them.</EmptyState>}
          </div>
        )}
        {tab === "agents" && <AgentsTable rows={owned.data} loading={owned.isLoading} error={owned.error} />}
        {tab === "tasks" && <><TasksTable rows={tasks.data?.rows} loading={tasks.isLoading} error={tasks.error} />{tasks.data && <Pagination index={index} more={tasks.data.more} />}</>}
        {tab === "channels" && <><ChannelsTable rows={channels.data?.rows} loading={channels.isLoading} error={channels.error} />{channels.data && <Pagination index={index} more={channels.data.more} />}</>}
      </Card>
    </Page>
  );
}

export function AccountsPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const { data, error, isLoading } = useRichList(index);
  return (
    <Page>
      <PageHeader title="Top accounts" icon={<Wallet size={20} />} subtitle="Accounts by total balance, free and reserved" />
      <Card>
        <Table
          rows={data?.rows}
          loading={isLoading}
          error={error}
          rowKey={(a) => a.address}
          columns={[
            { header: "Rank", cell: (a) => <span className="tabular text-muted">{index * 25 + (data?.rows.indexOf(a) ?? 0) + 1}</span> },
            { header: "Account", cell: (a) => <AddressLink address={a.address} /> },
            { header: "Balance", align: "right", cell: (a) => <Amount planck={a.total} decimals={2} /> },
            { header: "Reserved", align: "right", desktopOnly: true, cell: (a) => <Amount planck={a.reserved} decimals={2} /> },
            { header: "Share", align: "right", cell: (a) => <span className="tabular">{a.share != null ? `${Number(a.share).toFixed(2)}%` : "—"}</span> },
            { header: "Txs", align: "right", desktopOnly: true, cell: (a) => <span className="tabular">{formatNumber(a.nonce)}</span> },
          ]}
        />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

const WINDOWS = [
  { id: "1 day", label: "24 hours" },
  { id: "7 days", label: "7 days" },
  { id: "30 days", label: "30 days" },
  { id: "100 years", label: "All time" },
] as const;

export function MinersPage() {
  const [params] = useSearchParams();
  const window = params.get("window") ?? "";
  const { data, error, isLoading } = useTopMiners(window || "7 days");
  const top = data?.[0];
  return (
    <Page className="space-y-6">
      <PageHeader title="Miners" icon={<Pickaxe size={20} />} subtitle="Blocks found with RandomTESS, by miner. Every reward and fee goes to the miner.">
        <Filters name="window" value={window} options={WINDOWS.map((w) => ({ id: w.id === "7 days" ? "" : w.id, label: w.label }))} />
      </PageHeader>
      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
        <StatCard label="Active miners" loading={isLoading} value={formatNumber(data?.length ?? 0)} />
        <StatCard label="Blocks" loading={isLoading} value={formatNumber(data?.reduce((s, m) => s + Number(m.blocks), 0) ?? 0)} />
        <StatCard label="Largest miner" loading={isLoading} value={top ? `${Number(top.share).toFixed(1)}%` : "—"} sub={top ? shortAddress(top.author) : undefined} />
        <StatCard label="Paid to miners" loading={isLoading} value={<><Amount planck={(data ?? []).reduce((s, m) => s + BigInt(m.rewards) + BigInt(m.fees), 0n).toString()} decimals={0} unit={false} /> <span className="text-sm text-muted">TESS</span></>} />
      </div>
      <Card>
        <Table
          rows={data}
          loading={isLoading}
          error={error}
          rowKey={(m) => m.author}
          empty="No block mined in this period"
          columns={[
            { header: "Miner", cell: (m) => <AddressLink address={m.author} /> },
            { header: "Blocks", align: "right", cell: (m) => <span className="tabular">{formatNumber(m.blocks)}</span> },
            { header: "Share", cell: (m) => <div className="flex min-w-[140px] items-center gap-2"><Progress value={Number(m.share) / 100} className="flex-1" /><span className="w-14 text-right tabular text-sm">{Number(m.share).toFixed(2)}%</span></div> },
            { header: "Rewards", align: "right", cell: (m) => <Amount planck={m.rewards} decimals={2} /> },
            { header: "Fees", align: "right", desktopOnly: true, cell: (m) => <Amount planck={m.fees} decimals={4} /> },
            { header: "Last block", align: "right", desktopOnly: true, cell: (m) => <BlockLink number={Number(m.last_block)} /> },
          ]}
        />
      </Card>
      <p className="text-sm text-muted">
        Mine with the built-in miner of the node or with XMRig (algorithm rx/tess). See the{" "}
        <Link to="/supply" className="text-accent">emission schedule</Link> and the{" "}
        <a href="https://github.com/Wayleyy/tesserax-miner/releases" className="text-accent" target="_blank" rel="noreferrer">miner releases</a>.
      </p>
    </Page>
  );
}
