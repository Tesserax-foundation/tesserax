import { Coins, ScrollText, Zap } from "lucide-react";
import { Link, useParams, useSearchParams } from "react-router";
import { JsonView } from "../components/JsonView";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink, HashText, TimeAgo } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, EmptyState, ErrorState, Field, Fields, Filters, PageHeader, Pagination, Skeleton } from "../components/ui";
import { useEvents, useExtrinsic, useExtrinsics, useTransfers, type Transfer } from "../lib/api";
import { formatDate, formatNumber, humanize } from "../lib/format";
import { pageIndex } from "../lib/hooks";
import { BlockStatus, EventsList, ExtrinsicsTable } from "./Blocks";

const PALLETS = ["Balances", "Agents", "AgentRegistry", "Channels", "Tasks", "Shielded", "Revive", "Multisig", "Vesting"];

export function ExtrinsicsPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const pallet = params.get("pallet") ?? "";
  const { data, error, isLoading } = useExtrinsics(index, { pallet: pallet || undefined });
  return (
    <Page>
      <PageHeader title="Transactions" icon={<ScrollText size={20} />} subtitle="Signed extrinsics of the best chain" />
      <div className="mb-4">
        <Filters name="pallet" value={pallet} options={[{ id: "", label: "All" }, ...PALLETS.map((p) => ({ id: p, label: humanize(p) }))]} />
      </div>
      <Card>
        <ExtrinsicsTable rows={data?.rows} loading={isLoading} error={error} />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

export function ExtrinsicPage() {
  const { id = "" } = useParams();
  const { data, error, isLoading } = useExtrinsic(id);
  if (error) return <Page><Card><ErrorState error={error} /></Card></Page>;
  if (isLoading) return <Page><Skeleton className="mb-6 h-9 w-72" /><Card><div className="space-y-4 p-5">{Array.from({ length: 8 }, (_, i) => <Skeleton key={i} className="h-5 w-full" />)}</div></Card></Page>;
  if (!data) return <Page><Card><EmptyState title="Transaction not found">It may still be in the transaction pool, or not indexed yet.</EmptyState></Card></Page>;
  const { extrinsic: x, events, block } = data;
  return (
    <Page className="space-y-6">
      <PageHeader
        title={<>{humanize(x.pallet)} <span className="text-muted">·</span> {humanize(x.call)}</>}
        icon={<ScrollText size={20} />}
        subtitle={<span className="tabular">Extrinsic {x.block_number}-{x.index}</span>}
      >
        {x.success ? <Badge tone="success">Success</Badge> : <Badge tone="danger">Failed</Badge>}
      </PageHeader>
      <Card>
        <CardHeader title="Overview" />
        <Fields>
          <Field label="Hash"><HashText hash={x.hash} short={false} /></Field>
          <Field label="Block">
            <span className="inline-flex items-center gap-2">
              <BlockLink number={x.block_number} />
              {block && <BlockStatus block={block} />}
            </span>
          </Field>
          <Field label="Time">{formatDate(x.timestamp)} <TimeAgo date={x.timestamp} className="ml-1" /></Field>
          <Field label="Signer">{x.signer ? <AddressLink address={x.signer} full copy /> : <span className="text-muted">Inherent, added by the miner</span>}</Field>
          {x.nonce != null && <Field label="Nonce"><span className="tabular">{x.nonce}</span></Field>}
          <Field label="Result">{x.success ? <Badge tone="success">Success</Badge> : <span className="inline-flex flex-wrap items-center gap-2"><Badge tone="danger">Failed</Badge><code className="font-mono text-sm text-danger">{x.error}</code></span>}</Field>
          <Field label="Fee" hint="Paid to the miner of the block"><Amount planck={x.fee} decimals={12} /></Field>
          {x.tip && x.tip !== "0" && <Field label="Tip"><Amount planck={x.tip} decimals={12} /></Field>}
          <Field label="Size" hint="ML-DSA-87 signatures are 4,627 bytes and their public keys 2,592">{formatNumber(x.size)} bytes</Field>
          <Field label="Call"><Badge tone="accent">{x.pallet}.{x.call}</Badge></Field>
        </Fields>
      </Card>
      <Card>
        <CardHeader title="Arguments" />
        <div className="p-4 sm:p-5"><JsonView value={x.args} /></div>
      </Card>
      <Card>
        <CardHeader title="Events" icon={<Zap size={16} />}><Badge>{events.length}</Badge></CardHeader>
        <EventsList events={events} showExtrinsic={false} />
      </Card>
    </Page>
  );
}

export function TransfersTable({ rows, loading, error, address }: { rows: Transfer[] | undefined; loading?: boolean; error?: unknown; address?: string }) {
  return (
    <Table
      rows={rows}
      loading={loading}
      error={error}
      rowKey={(t) => `${t.block_hash}-${t.event_index}`}
      empty="No transfer"
      columns={[
        { header: "Extrinsic", cell: (t) => (t.extrinsic_index != null ? <Link to={`/extrinsic/${t.block_number}-${t.extrinsic_index}`} className="tabular text-accent">{t.block_number}-{t.extrinsic_index}</Link> : <BlockLink number={t.block_number} />) },
        { header: "Age", cell: (t) => <TimeAgo date={t.timestamp} /> },
        { header: "From", cell: (t) => <AddressLink address={t.from_address} /> },
        ...(address ? [{ header: "", cell: (t: Transfer) => (t.from_address === address ? <Badge tone="warning">Out</Badge> : <Badge tone="success">In</Badge>) }] : []),
        { header: "To", cell: (t) => <AddressLink address={t.to_address} /> },
        { header: "Amount", align: "right", cell: (t) => <Amount planck={t.amount} /> },
      ]}
    />
  );
}

export function TransfersPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const { data, error, isLoading } = useTransfers(index);
  return (
    <Page>
      <PageHeader title="Transfers" icon={<Coins size={20} />} subtitle="TESS moving between accounts" />
      <Card>
        <TransfersTable rows={data?.rows} loading={isLoading} error={error} />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

export function EventsPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const pallet = params.get("pallet") ?? "";
  const { data, error, isLoading } = useEvents(index, { pallet: pallet || undefined });
  return (
    <Page>
      <PageHeader title="Events" icon={<Zap size={20} />} subtitle="What the runtime emitted, block by block" />
      <div className="mb-4">
        <Filters name="pallet" value={pallet} options={[{ id: "", label: "All" }, ...["System", "Emission", ...PALLETS, "TransactionPayment"].map((p) => ({ id: p, label: humanize(p) }))]} />
      </div>
      <Card>
        {error ? <ErrorState error={error} /> : isLoading ? <div className="space-y-3 p-5">{Array.from({ length: 8 }, (_, i) => <Skeleton key={i} className="h-5 w-full" />)}</div> : <EventsList events={data?.rows ?? []} />}
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}
