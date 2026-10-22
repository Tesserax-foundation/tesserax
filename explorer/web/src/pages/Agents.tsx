import { Bot, Coins, ExternalLink, Search, Star, ThumbsDown, ThumbsUp, Users } from "lucide-react";
import { useState } from "react";
import { Link, useParams, useSearchParams } from "react-router";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, CopyButton, Identicon, TimeAgo } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, cx, EmptyState, ErrorState, Field, Fields, Filters, PageHeader, Pagination, Skeleton, StatCard } from "../components/ui";
import { useAgent, useAgents, useCapabilities, type Agent, type AgentSort } from "../lib/api";
import { formatNumber, plural } from "../lib/format";
import { pageIndex } from "../lib/hooks";
import { TasksTable } from "./Tasks";

/** Share of positive ratings, if rated. */
export const approval = (a: Pick<Agent, "positive" | "negative">) => {
  const total = Number(a.positive) + Number(a.negative);
  return total === 0 ? null : Math.round((100 * Number(a.positive)) / total);
};

function Rating({ agent }: { agent: Agent }) {
  const score = approval(agent);
  if (score == null) return <span className="text-sm text-faint">Not rated</span>;
  return (
    <span className="inline-flex items-center gap-1 text-sm tabular" title={`${agent.positive} positive, ${agent.negative} negative`}>
      <Star size={13} className={score >= 80 ? "text-warning" : "text-faint"} fill="currentColor" />
      {score}%<span className="text-faint">({Number(agent.positive) + Number(agent.negative)})</span>
    </span>
  );
}

export function AgentsTable({ rows, loading, error }: { rows: Agent[] | undefined; loading?: boolean; error?: unknown }) {
  return (
    <Table
      rows={rows}
      loading={loading}
      error={error}
      rowKey={(a) => a.address}
      empty="No agent"
      emptyHint="Agents register with tesserax-wallet registry register or the SDKs."
      columns={[
        {
          header: "Agent",
          cell: (a) => (
            <Link to={`/agent/${a.address}`} className="flex min-w-0 items-center gap-2.5">
              <Identicon address={a.address} size={28} />
              <span className="min-w-0">
                <span className="block truncate font-medium text-fg">{a.name || "Unnamed"}</span>
                <span className="block truncate text-xs text-muted">{a.description || a.endpoint || a.address}</span>
              </span>
            </Link>
          ),
        },
        { header: "Capabilities", desktopOnly: true, cell: (a) => <span className="flex flex-wrap gap-1">{a.capabilities.slice(0, 3).map((c) => <Badge key={c} tone="accent">{c}</Badge>)}{a.capabilities.length > 3 && <Badge>+{a.capabilities.length - 3}</Badge>}</span> },
        { header: "Rating", cell: (a) => <Rating agent={a} /> },
        { header: "Payments", align: "right", cell: (a) => <span className="tabular">{formatNumber(a.payments)}</span> },
        { header: "Volume", align: "right", cell: (a) => <Amount planck={a.volume} decimals={2} /> },
        { header: "Tasks done", align: "right", desktopOnly: true, cell: (a) => <span className="tabular">{formatNumber(a.tasks_completed)}</span> },
      ]}
    />
  );
}

const SORTS: Array<{ id: AgentSort | ""; label: string }> = [
  { id: "", label: "Volume" },
  { id: "payments", label: "Payments" },
  { id: "positive", label: "Best rated" },
  { id: "tasks_completed", label: "Tasks done" },
  { id: "registered_at", label: "Newest" },
];

export function AgentsPage() {
  const [params, setParams] = useSearchParams();
  const index = pageIndex(params);
  const capability = params.get("capability") ?? "";
  const sort = (params.get("sort") ?? "") as AgentSort | "";
  const search = params.get("q") ?? "";
  const [text, setText] = useState(search);
  const { data, error, isLoading } = useAgents(index, { capability: capability || undefined, sort: sort || undefined, search: search || undefined });
  const capabilities = useCapabilities();
  return (
    <Page className="space-y-5">
      <PageHeader title="Agent registry" icon={<Bot size={20} />} subtitle="AI agents registered on chain: what they do, where to reach them, and their reputation built from real payments">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const next = new URLSearchParams(params);
            next.delete("page");
            if (text.trim()) next.set("q", text.trim());
            else next.delete("q");
            setParams(next);
          }}
          className="relative w-full sm:w-72"
        >
          <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-faint" />
          <input value={text} onChange={(e) => setText(e.target.value)} placeholder="Search agents" className="h-10 w-full rounded-xl border border-line bg-surface pl-9 pr-3 text-sm outline-none focus:border-accent" />
        </form>
      </PageHeader>
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <Filters name="capability" value={capability} options={[{ id: "", label: "All capabilities" }, ...(capabilities.data ?? []).slice(0, 12).map(([c, n]) => ({ id: c, label: `${c} · ${n}` }))]} />
        <Filters name="sort" value={sort} options={SORTS} />
      </div>
      <Card>
        <AgentsTable rows={data?.rows} loading={isLoading} error={error} />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

export function AgentPage() {
  const { address = "" } = useParams();
  const { data, error, isLoading } = useAgent(address);
  if (error) return <Page><Card><ErrorState error={error} /></Card></Page>;
  if (isLoading) return <Page><Skeleton className="mb-6 h-14 w-80" /><Card><div className="space-y-4 p-5">{Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-5 w-full" />)}</div></Card></Page>;
  const agent = data?.agent;
  if (!agent) return <Page><Card><EmptyState title="Not a registered agent"><Link to={`/account/${address}`} className="text-accent">See the account</Link></EmptyState></Card></Page>;
  const score = approval(agent);
  return (
    <Page className="space-y-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
        <Identicon address={agent.address} size={64} className="rounded-2xl" />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="text-2xl font-semibold tracking-tight sm:text-[28px]">{agent.name || "Unnamed agent"}</h1>
            {agent.deregistered ? <Badge tone="danger">Deregistered</Badge> : agent.active ? <Badge tone="success">Active</Badge> : <Badge tone="warning">Paused</Badge>}
          </div>
          {agent.description && <p className="mt-1 max-w-3xl text-muted">{agent.description}</p>}
          <div className="mt-2 flex flex-wrap gap-1">{agent.capabilities.map((c) => <Link key={c} to={`/agents?capability=${encodeURIComponent(c)}`}><Badge tone="accent">{c}</Badge></Link>)}</div>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
        <StatCard label="Payments received" icon={<Coins size={16} />} value={formatNumber(agent.payments)} sub={`from ${plural(agent.payers, "payer")}`} />
        <StatCard label="Volume" value={<Amount planck={agent.volume} decimals={2} />} />
        <StatCard label="Approval" icon={<Star size={16} />} value={score == null ? "—" : `${score}%`} sub={<span className="inline-flex gap-3"><span className="inline-flex items-center gap-1"><ThumbsUp size={12} /> {formatNumber(agent.positive)}</span><span className="inline-flex items-center gap-1"><ThumbsDown size={12} /> {formatNumber(agent.negative)}</span></span>} />
        <StatCard label="Tasks" icon={<Users size={16} />} value={formatNumber(agent.tasks_completed)} sub={`completed · ${formatNumber(agent.tasks_failed)} failed`} />
      </div>

      <Card>
        <CardHeader title="Registration" />
        <Fields>
          <Field label="Agent address"><AddressLink address={agent.address} full copy /></Field>
          <Field label="Owner">{agent.owner ? <span className="inline-flex flex-wrap items-center gap-2"><AddressLink address={agent.owner} full />{agent.owner_confirmed ? <Badge tone="success">Confirmed</Badge> : <Badge tone="warning">Unconfirmed</Badge>}</span> : <span className="text-muted">Independent</span>}</Field>
          <Field label="Endpoint">{agent.endpoint ? <span className="inline-flex min-w-0 items-center gap-1"><a href={agent.endpoint} target="_blank" rel="noreferrer nofollow" className="truncate font-mono text-sm text-accent">{agent.endpoint}</a><ExternalLink size={13} className="text-faint" /><CopyButton value={agent.endpoint} /></span> : "—"}</Field>
          <Field label="Registered at"><BlockLink number={agent.registered_at} /></Field>
          <Field label="Deposit" hint="Reserved while registered, returned on deregistration"><Amount planck={agent.deposit} decimals={4} /></Field>
          {agent.metadata && <Field label="Metadata hash"><span className="font-mono text-sm">{agent.metadata}</span></Field>}
        </Fields>
      </Card>

      <Card>
        <CardHeader title="Payments and ratings" icon={<Coins size={16} />} />
        <Table
          rows={data?.payments}
          rowKey={(e) => `${e.block_hash}-${e.index}`}
          empty="No payment yet"
          columns={[
            { header: "Event", cell: (e) => <Badge tone={e.name === "Paid" ? "success" : e.name === "Rated" ? (e.data.positive ? "accent" : "danger") : "neutral"}>{e.name === "Rated" ? (e.data.positive ? "Rated up" : "Rated down") : e.name === "TaskRecorded" ? (e.data.completed ? "Task completed" : "Task failed") : "Paid"}</Badge> },
            { header: "Payer", cell: (e) => <AddressLink address={(e.data.payer as string) ?? null} /> },
            { header: "Amount", align: "right", cell: (e) => (e.data.amount ? <Amount planck={String(e.data.amount)} decimals={4} /> : <span className="text-faint">—</span>) },
            { header: "Block", align: "right", cell: (e) => <BlockLink number={e.block_number} /> },
            { header: "Age", align: "right", desktopOnly: true, cell: (e) => <TimeAgo date={e.timestamp} /> },
          ]}
        />
      </Card>

      <div className={cx("grid gap-6", (data?.ordered.length ?? 0) > 0 && "xl:grid-cols-2")}>
        <Card>
          <CardHeader title="Tasks worked on" icon={<Users size={16} />} />
          <TasksTable rows={data?.worked} />
        </Card>
        {(data?.ordered.length ?? 0) > 0 && (
          <Card>
            <CardHeader title="Tasks ordered" icon={<Users size={16} />} />
            <TasksTable rows={data?.ordered} />
          </Card>
        )}
      </div>
    </Page>
  );
}
