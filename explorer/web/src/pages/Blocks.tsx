import { Blocks as BlocksIcon, ChevronLeft, ChevronRight, ScrollText, Zap } from "lucide-react";
import { useState } from "react";
import { Link, useParams, useSearchParams } from "react-router";
import { JsonView } from "../components/JsonView";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, HashText, TimeAgo } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, EmptyState, ErrorState, Field, Fields, Filters, PageHeader, Pagination, Skeleton, Tabs } from "../components/ui";
import { useBlock, useBlocks, useChainState, type Block, type ChainEvent, type Extrinsic } from "../lib/api";
import { callLabel, formatCompact, formatDate, formatNumber } from "../lib/format";
import { pageIndex } from "../lib/hooks";

export const signedCount = (b: Pick<Block, "extrinsic_count" | "number">) => Math.max(0, b.extrinsic_count - (b.number > 0 ? 1 : 0));

export function BlockStatus({ block, best }: { block: Block; best?: number }) {
  if (!block.canonical) return <Badge tone="warning">Orphaned</Badge>;
  if (block.finalized) return <Badge tone="success">Final</Badge>;
  const confirmations = best != null ? best - block.number + 1 : null;
  return <Badge tone="accent">{confirmations != null ? `${confirmations} confirmation${confirmations === 1 ? "" : "s"}` : "Unfinalized"}</Badge>;
}

export function BlocksPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const orphaned = params.get("orphaned") === "1";
  const { data, error, isLoading } = useBlocks(index, { orphaned });
  const head = useChainState().data?.head;
  return (
    <Page>
      <PageHeader title={orphaned ? "Orphaned blocks" : "Blocks"} icon={<BlocksIcon size={20} />} subtitle={orphaned ? "Blocks mined on a branch that lost to a heavier chain" : head ? `${formatNumber(head.best + 1)} blocks, final up to #${formatNumber(head.finalized)}` : undefined}>
        <Filters name="orphaned" value={orphaned ? "1" : ""} options={[{ id: "", label: "Best chain" }, { id: "1", label: "Orphaned" }]} />
      </PageHeader>
      <Card>
        <Table
          rows={data?.rows}
          loading={isLoading}
          error={error}
          rowKey={(b) => b.hash}
          empty={orphaned ? "No orphaned block" : "No block yet"}
          columns={[
            { header: "Block", cell: (b) => (orphaned ? <Link to={`/block/${b.hash}`} className="font-medium text-accent">#{formatNumber(b.number)}</Link> : <BlockLink number={b.number} />) },
            { header: "Age", cell: (b) => <TimeAgo date={b.timestamp} /> },
            { header: "Miner", cell: (b) => <AddressLink address={b.author} /> },
            { header: "Txs", align: "right", cell: (b) => formatNumber(signedCount(b)) },
            { header: "Events", align: "right", desktopOnly: true, cell: (b) => formatNumber(b.event_count) },
            { header: "Difficulty", align: "right", desktopOnly: true, cell: (b) => formatCompact(b.difficulty) },
            { header: "Reward", align: "right", cell: (b) => <Amount planck={(BigInt(b.reward) + BigInt(b.fees)).toString()} decimals={3} /> },
            { header: "Status", align: "right", desktopOnly: true, cell: (b) => <BlockStatus block={b} best={head?.best} /> },
          ]}
        />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

export function ExtrinsicsTable({ rows, loading, error, showBlock = true }: { rows: Extrinsic[] | undefined; loading?: boolean; error?: unknown; showBlock?: boolean }) {
  return (
    <Table
      rows={rows}
      loading={loading}
      error={error}
      rowKey={(x) => `${x.block_hash}-${x.index}`}
      empty="No transaction"
      columns={[
        { header: "Hash", cell: (x) => <HashText hash={x.hash} to={`/extrinsic/${x.hash}`} copy={false} /> },
        ...(showBlock ? [{ header: "Block", cell: (x: Extrinsic) => <Link to={`/extrinsic/${x.block_number}-${x.index}`} className="tabular text-accent">{x.block_number}-{x.index}</Link> }] : [{ header: "Index", cell: (x: Extrinsic) => <span className="tabular">{x.index}</span> }]),
        { header: "Call", cell: (x) => <Badge>{callLabel(x.pallet, x.call)}</Badge> },
        { header: "Signer", cell: (x) => (x.signer ? <AddressLink address={x.signer} /> : <span className="text-faint">Inherent</span>) },
        { header: "Fee", align: "right", desktopOnly: true, cell: (x) => <Amount planck={x.fee} decimals={6} /> },
        { header: "Result", align: "right", cell: (x) => (x.success ? <Badge tone="success">Success</Badge> : <Badge tone="danger" className="max-w-[220px] truncate">{x.error ?? "Failed"}</Badge>) },
        ...(showBlock ? [{ header: "Age", align: "right" as const, desktopOnly: true, cell: (x: Extrinsic) => <TimeAgo date={x.timestamp} /> }] : []),
      ]}
    />
  );
}

export function EventsList({ events, showExtrinsic = true }: { events: ChainEvent[]; showExtrinsic?: boolean }) {
  if (events.length === 0) return <EmptyState title="No event" />;
  return (
    <ul className="divide-y divide-line">
      {events.map((e) => (
        <EventItem key={`${e.block_hash}-${e.index}`} event={e} showExtrinsic={showExtrinsic} />
      ))}
    </ul>
  );
}

function EventItem({ event, showExtrinsic }: { event: ChainEvent; showExtrinsic: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <li className="px-4 py-3 sm:px-5">
      <button onClick={() => setOpen(!open)} className="flex w-full items-center gap-3 text-left">
        <Zap size={15} className="shrink-0 text-accent" />
        <span className="min-w-0 flex-1 truncate text-sm font-medium">
          {event.pallet}
          <span className="text-muted"> · </span>
          {event.name}
        </span>
        {showExtrinsic && (
          <span className="shrink-0 text-xs text-muted tabular">
            {event.extrinsic_index != null ? `Extrinsic ${event.block_number}-${event.extrinsic_index}` : "Block"} · #{event.index}
          </span>
        )}
        <ChevronRight size={15} className={`shrink-0 text-faint transition-transform ${open ? "rotate-90" : ""}`} />
      </button>
      {open && (
        <div className="mt-3">
          <JsonView value={event.data} />
        </div>
      )}
    </li>
  );
}

export function BlockPage() {
  const { id = "" } = useParams();
  const { data, error, isLoading } = useBlock(id);
  const head = useChainState().data?.head;
  const [tab, setTab] = useState<"extrinsics" | "events">("extrinsics");

  if (error) return <Page><Card><ErrorState error={error} /></Card></Page>;
  if (isLoading) return <Page><Skeleton className="mb-6 h-9 w-56" /><Card><div className="space-y-4 p-5">{Array.from({ length: 8 }, (_, i) => <Skeleton key={i} className="h-5 w-full" />)}</div></Card></Page>;
  if (!data) return <Page><Card><EmptyState title="Block not found">It may not be indexed yet, or it does not exist.</EmptyState></Card></Page>;

  const { block, extrinsics, events, siblings } = data;
  return (
    <Page className="space-y-6">
      <PageHeader title={<>Block <span className="tabular">#{formatNumber(block.number)}</span></>} icon={<BlocksIcon size={20} />} subtitle={<BlockStatus block={block} best={head?.best} />}>
        <div className="flex gap-2">
          {block.number > 0 && (
            <Link to={`/block/${block.number - 1}`} className="inline-flex h-9 items-center gap-1 rounded-lg border border-line bg-surface px-3 text-sm font-medium hover:border-line-strong">
              <ChevronLeft size={16} /> Previous
            </Link>
          )}
          {head && block.number < head.best && (
            <Link to={`/block/${block.number + 1}`} className="inline-flex h-9 items-center gap-1 rounded-lg border border-line bg-surface px-3 text-sm font-medium hover:border-line-strong">
              Next <ChevronRight size={16} />
            </Link>
          )}
        </div>
      </PageHeader>

      {!block.canonical && (
        <div className="rounded-xl border border-warning/30 bg-warning-soft px-4 py-3 text-sm text-warning">
          This block was mined but a heavier chain replaced it: its transactions are not part of the chain.
        </div>
      )}

      <Card>
        <CardHeader title="Overview" />
        <Fields>
          <Field label="Hash"><HashText hash={block.hash} short={false} /></Field>
          <Field label="Time">{block.timestamp ? <>{formatDate(block.timestamp)} <TimeAgo date={block.timestamp} className="ml-1" /></> : "Genesis"}</Field>
          <Field label="Miner"><AddressLink address={block.author} full copy /></Field>
          <Field label="Reward"><Amount planck={block.reward} decimals={12} /></Field>
          <Field label="Fees" hint="Every fee goes to the miner: nothing is burned"><Amount planck={block.fees} decimals={12} /></Field>
          <Field label="Transactions">{formatNumber(signedCount(block))} signed, {formatNumber(block.extrinsic_count)} extrinsics, {formatNumber(block.event_count)} events, {formatNumber(block.transfer_count)} transfers</Field>
          <Field label="Difficulty" hint="Expected number of RandomTESS hashes to find this block">{block.difficulty ? formatNumber(block.difficulty) : "—"}</Field>
          <Field label="Proof of work" hint="Nonce and RandomTESS hash of the seal">
            {block.pow_nonce ? (
              <div className="space-y-1">
                <div className="flex items-center gap-2"><span className="w-16 shrink-0 text-xs text-muted">Nonce</span><HashText hash={block.pow_nonce} short={false} /></div>
                <div className="flex items-center gap-2"><span className="w-16 shrink-0 text-xs text-muted">Hash</span><HashText hash={block.pow_hash} /></div>
              </div>
            ) : "—"}
          </Field>
          <Field label="Parent">{block.number > 0 ? <HashText hash={block.parent_hash} to={`/block/${block.parent_hash}`} /> : "—"}</Field>
          <Field label="State root"><HashText hash={block.state_root} /></Field>
          <Field label="Extrinsics root"><HashText hash={block.extrinsics_root} /></Field>
          <Field label="Size">{formatNumber(block.size)} bytes</Field>
          <Field label="Runtime">Spec version {block.spec_version}</Field>
          {siblings.length > 0 && (
            <Field label="Other blocks at this height">
              <div className="space-y-1">
                {siblings.map((s) => (
                  <div key={s.hash} className="flex items-center gap-2">
                    <HashText hash={s.hash} to={`/block/${s.hash}`} />
                    {s.canonical ? <Badge tone="success">Best chain</Badge> : <Badge tone="warning">Orphaned</Badge>}
                  </div>
                ))}
              </div>
            </Field>
          )}
        </Fields>
      </Card>

      <Card>
        <div className="border-b border-line">
          <Tabs
            value={tab}
            onChange={setTab}
            tabs={[
              { id: "extrinsics", label: <><ScrollText size={15} /> Extrinsics</>, count: extrinsics.length },
              { id: "events", label: <><Zap size={15} /> Events</>, count: events.length },
            ]}
          />
        </div>
        {tab === "extrinsics" ? (
          block.canonical ? <ExtrinsicsTable rows={extrinsics} showBlock={false} /> : <EmptyState title="Transactions of orphaned blocks are not indexed" />
        ) : (
          <EventsList events={events} />
        )}
      </Card>
    </Page>
  );
}
