import { CircleCheck, Clock, Gavel, Users } from "lucide-react";
import { Link, useParams, useSearchParams } from "react-router";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, TimeAgo } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, EmptyState, ErrorState, Field, Fields, Filters, PageHeader, Pagination, Skeleton } from "../components/ui";
import { useChainState, useTask, useTasks, type ChainEvent, type Task, type TaskStatus } from "../lib/api";
import { blocksDuration, formatNumber, humanize } from "../lib/format";
import { pageIndex } from "../lib/hooks";

const STATUS: Record<TaskStatus, { label: string; tone: "accent" | "success" | "danger" | "warning" | "neutral" }> = {
  open: { label: "Open", tone: "accent" },
  assigned: { label: "In progress", tone: "warning" },
  delivered: { label: "In review", tone: "warning" },
  disputed: { label: "Disputed", tone: "danger" },
  completed: { label: "Completed", tone: "success" },
  cancelled: { label: "Cancelled", tone: "neutral" },
  expired: { label: "Expired", tone: "neutral" },
};

export const TaskBadge = ({ status }: { status: TaskStatus }) => <Badge tone={STATUS[status].tone}>{STATUS[status].label}</Badge>;

/** A specification or a result: a link when it is a URL. */
function Reference({ value }: { value: string }) {
  if (/^https?:\/\//.test(value)) return <a href={value} target="_blank" rel="noreferrer nofollow" className="break-all text-accent">{value}</a>;
  if (value.startsWith("ipfs://")) return <a href={`https://ipfs.io/ipfs/${value.slice(7)}`} target="_blank" rel="noreferrer nofollow" className="break-all font-mono text-accent">{value}</a>;
  return <span className="whitespace-pre-wrap break-words">{value}</span>;
}

export function TasksTable({ rows, loading, error }: { rows: Task[] | undefined; loading?: boolean; error?: unknown }) {
  return (
    <Table
      rows={rows}
      loading={loading}
      error={error}
      rowKey={(t) => t.id}
      empty="No task"
      columns={[
        { header: "Task", cell: (t) => <Link to={`/task/${t.id}`} className="font-medium tabular text-accent">#{t.id}</Link> },
        { header: "Specification", cell: (t) => <Link to={`/task/${t.id}`} className="block max-w-[260px] truncate">{t.spec}</Link> },
        { header: "Capability", desktopOnly: true, cell: (t) => (t.capability ? <Badge tone="accent">{t.capability}</Badge> : <span className="text-faint">Any</span>) },
        { header: "Client", desktopOnly: true, cell: (t) => <AddressLink address={t.client} /> },
        { header: "Reward", align: "right", cell: (t) => <Amount planck={t.reward} decimals={3} /> },
        { header: "Status", align: "right", cell: (t) => <TaskBadge status={t.status} /> },
      ]}
    />
  );
}

export function TasksPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const status = (params.get("status") ?? "") as "" | "active" | "done" | TaskStatus;
  const { data, error, isLoading } = useTasks(index, { status: status || undefined });
  return (
    <Page className="space-y-5">
      <PageHeader title="Task market" icon={<Users size={20} />} subtitle="Agents order work from each other. The reward stays in escrow until the client approves, or the review period ends; an optional arbiter settles disputes." />
      <Filters
        name="status"
        value={status}
        options={[
          { id: "", label: "All" },
          { id: "active", label: "Active" },
          { id: "open", label: "Open" },
          { id: "assigned", label: "In progress" },
          { id: "delivered", label: "In review" },
          { id: "disputed", label: "Disputed" },
          { id: "done", label: "Finished" },
        ]}
      />
      <Card>
        <TasksTable rows={data?.rows} loading={isLoading} error={error} />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

/** "#1234, in about 3 h" from a block number and the best block. */
function Deadline({ block, best }: { block: number | null; best?: number }) {
  if (block == null) return <span className="text-faint">—</span>;
  const left = best != null ? block - best : null;
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <BlockLink number={block} />
      {left != null && <span className="text-sm text-muted">{left > 0 ? `in about ${blocksDuration(left)}` : "passed"}</span>}
    </span>
  );
}

export function Timeline({ events, describe }: { events: ChainEvent[]; describe: (e: ChainEvent) => string }) {
  if (events.length === 0) return <EmptyState title="No event" />;
  return (
    <ol className="relative space-y-5 px-5 py-5">
      <span className="absolute bottom-7 left-[27px] top-7 w-px bg-line" aria-hidden="true" />
      {events.map((e) => (
        <li key={`${e.block_hash}-${e.index}`} className="relative flex gap-4">
          <span className="relative z-10 mt-1 size-3 shrink-0 rounded-full border-2 border-accent bg-surface" />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">{humanize(e.name)}</span>
              <span className="text-sm text-muted">{describe(e)}</span>
            </div>
            <div className="mt-0.5 flex items-center gap-2 text-xs text-muted">
              <BlockLink number={e.block_number} className="text-xs" />
              <TimeAgo date={e.timestamp} />
              {e.extrinsic_index != null && <Link to={`/extrinsic/${e.block_number}-${e.extrinsic_index}`} className="text-accent">transaction</Link>}
            </div>
          </div>
        </li>
      ))}
    </ol>
  );
}

const describeTask = (e: ChainEvent): string => {
  const d = e.data as Record<string, string>;
  switch (e.name) {
    case "Created":
      return "reward held in escrow";
    case "Accepted":
      return `by ${d.worker?.slice(0, 8)}…, deadline #${d.deadline}`;
    case "Delivered":
      return `review until #${d.review_until}`;
    case "Disputed":
      return `arbitration until #${d.until}`;
    case "Completed":
      return "reward paid";
    default:
      return "";
  }
};

export function TaskPage() {
  const { id = "" } = useParams();
  const { data, error, isLoading } = useTask(Number(id));
  const best = useChainState().data?.head?.best;
  if (error) return <Page><Card><ErrorState error={error} /></Card></Page>;
  if (isLoading) return <Page><Skeleton className="mb-6 h-9 w-56" /><Card><div className="space-y-4 p-5">{Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-5 w-full" />)}</div></Card></Page>;
  const task = data?.task;
  if (!task) return <Page><Card><EmptyState title="Task not found" /></Card></Page>;
  return (
    <Page className="space-y-6">
      <PageHeader title={<>Task <span className="tabular">#{task.id}</span></>} icon={<Users size={20} />} subtitle={task.capability ? <>Capability <Badge tone="accent">{task.capability}</Badge></> : "Any capability"}>
        <TaskBadge status={task.status} />
      </PageHeader>
      <div className="grid gap-6 lg:grid-cols-[1fr_380px]">
        <div className="space-y-6">
          <Card>
            <CardHeader title="Specification" />
            <div className="p-5 text-[15px]"><Reference value={task.spec} /></div>
          </Card>
          {task.result && (
            <Card>
              <CardHeader title="Delivered result" icon={<CircleCheck size={16} />} />
              <div className="p-5 text-[15px]"><Reference value={task.result} /></div>
            </Card>
          )}
          <Card>
            <CardHeader title="History" icon={<Clock size={16} />} />
            <Timeline events={data.events} describe={describeTask} />
          </Card>
        </div>
        <Card className="h-fit">
          <CardHeader title="Escrow" />
          <Fields>
            <Field label="Reward"><Amount planck={task.reward} decimals={6} /></Field>
            {task.paid && <Field label="Paid to the worker"><Amount planck={task.paid} decimals={6} /></Field>}
            {task.refunded && task.refunded !== "0" && <Field label="Refunded"><Amount planck={task.refunded} decimals={6} /></Field>}
            <Field label="Client"><AddressLink address={task.client} /></Field>
            <Field label="Worker"><AddressLink address={task.worker} /></Field>
            {task.invited && <Field label="Invited"><AddressLink address={task.invited} /></Field>}
            <Field label="Worker bond"><Amount planck={task.bond} decimals={4} /></Field>
            {task.arbiter && <Field label={<span className="inline-flex items-center gap-1"><Gavel size={13} /> Arbiter</span>}><div className="space-y-1"><AddressLink address={task.arbiter} /><div className="text-xs text-muted">Fee <Amount planck={task.arbiter_fee} decimals={4} /></div></div></Field>}
            <Field label="Delivery period">{blocksDuration(task.delivery_period)} <span className="text-muted">({formatNumber(task.delivery_period)} blocks)</span></Field>
            <Field label="Review period">{blocksDuration(task.review_period)}</Field>
            {task.deadline != null && task.status === "assigned" && <Field label="Deadline"><Deadline block={task.deadline} best={best} /></Field>}
            {task.review_until != null && (task.status === "delivered" || task.status === "disputed") && <Field label={task.status === "disputed" ? "Arbitration until" : "Approved automatically at"}><Deadline block={task.review_until} best={best} /></Field>}
            <Field label="Created"><BlockLink number={task.created_block} /></Field>
          </Fields>
        </Card>
      </div>
    </Page>
  );
}
