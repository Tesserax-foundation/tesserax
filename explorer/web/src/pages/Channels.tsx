import { Clock, Zap } from "lucide-react";
import { Link, useParams, useSearchParams } from "react-router";
import { Progress } from "../components/charts";
import { Page } from "../components/Layout";
import { AddressLink, Amount, BlockLink } from "../components/links";
import { Table } from "../components/Table";
import { Badge, Card, CardHeader, EmptyState, ErrorState, Field, Fields, Filters, PageHeader, Pagination, Skeleton } from "../components/ui";
import { useChainState, useChannel, useChannels, type Channel, type ChainEvent } from "../lib/api";
import { blocksDuration, formatTess } from "../lib/format";
import { pageIndex } from "../lib/hooks";
import { Timeline } from "./Tasks";

export function ChannelBadge({ status }: { status: Channel["status"] }) {
  return status === "open" ? <Badge tone="success">Open</Badge> : status === "closing" ? <Badge tone="warning">Closing</Badge> : <Badge>Closed</Badge>;
}

const used = (c: Channel) => {
  const deposit = BigInt(c.deposit);
  return deposit === 0n ? 0 : Number((BigInt(c.claimed) * 10000n) / deposit) / 10000;
};

export function ChannelsTable({ rows, loading, error }: { rows: Channel[] | undefined; loading?: boolean; error?: unknown }) {
  return (
    <Table
      rows={rows}
      loading={loading}
      error={error}
      rowKey={(c) => c.id}
      empty="No payment channel"
      columns={[
        { header: "Channel", cell: (c) => <Link to={`/channel/${c.id}`} className="font-medium tabular text-accent">#{c.id}</Link> },
        { header: "Payer", cell: (c) => <AddressLink address={c.payer} /> },
        { header: "Payee", cell: (c) => <AddressLink address={c.payee} /> },
        { header: "Deposit", align: "right", cell: (c) => <Amount planck={c.deposit} decimals={3} /> },
        { header: "Claimed", align: "right", desktopOnly: true, cell: (c) => <div className="ml-auto flex w-36 items-center gap-2"><Progress value={used(c)} className="flex-1" /><Amount planck={c.claimed} decimals={2} unit={false} /></div> },
        { header: "Status", align: "right", cell: (c) => <ChannelBadge status={c.status} /> },
      ]}
    />
  );
}

export function ChannelsPage() {
  const [params] = useSearchParams();
  const index = pageIndex(params);
  const status = (params.get("status") ?? "") as "" | Channel["status"];
  const { data, error, isLoading } = useChannels(index, { status: status || undefined });
  return (
    <Page className="space-y-5">
      <PageHeader title="Payment channels" icon={<Zap size={20} />} subtitle="Micropayments without a transaction per payment: the payer signs cumulative vouchers off chain, the payee claims them from time to time." />
      <Filters name="status" value={status} options={[{ id: "", label: "All" }, { id: "open", label: "Open" }, { id: "closing", label: "Closing" }, { id: "closed", label: "Closed" }]} />
      <Card>
        <ChannelsTable rows={data?.rows} loading={isLoading} error={error} />
        {data && <Pagination index={index} more={data.more} />}
      </Card>
    </Page>
  );
}

const describeChannel = (e: ChainEvent): string => {
  const d = e.data as Record<string, string>;
  switch (e.name) {
    case "Opened":
      return `deposit ${formatTess(d.deposit)} TESS`;
    case "ToppedUp":
      return `deposit now ${formatTess(d.deposit)} TESS`;
    case "Claimed":
      return `${formatTess(d.paid)} TESS paid, ${formatTess(d.claimed)} in total`;
    case "CloseRequested":
      return `closes at #${d.closes_at}`;
    case "Closed":
      return `${formatTess(d.refunded)} TESS back to the payer`;
    default:
      return "";
  }
};

export function ChannelPage() {
  const { id = "" } = useParams();
  const { data, error, isLoading } = useChannel(Number(id));
  const best = useChainState().data?.head?.best;
  if (error) return <Page><Card><ErrorState error={error} /></Card></Page>;
  if (isLoading) return <Page><Skeleton className="mb-6 h-9 w-56" /><Card><div className="space-y-4 p-5">{Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-5 w-full" />)}</div></Card></Page>;
  const channel = data?.channel;
  if (!channel) return <Page><Card><EmptyState title="Channel not found" /></Card></Page>;
  const remaining = BigInt(channel.deposit) - BigInt(channel.claimed);
  return (
    <Page className="space-y-6">
      <PageHeader title={<>Channel <span className="tabular">#{channel.id}</span></>} icon={<Zap size={20} />}>
        <ChannelBadge status={channel.status} />
      </PageHeader>
      <div className="grid gap-6 lg:grid-cols-[1fr_380px]">
        <Card>
          <CardHeader title="History" icon={<Clock size={16} />} />
          <Timeline events={data.events} describe={describeChannel} />
        </Card>
        <Card className="h-fit">
          <CardHeader title="Channel" />
          <Fields narrow>
            <Field label="Payer"><AddressLink address={channel.payer} /></Field>
            <Field label="Payee"><AddressLink address={channel.payee} /></Field>
            <Field label="Voucher signer" hint="Key whose ML-DSA-87 signatures the vouchers carry, an agent key in delegated mode"><AddressLink address={channel.signer} /></Field>
            <Field label="Deposit"><Amount planck={channel.deposit} decimals={6} /></Field>
            <Field label="Claimed">
              <div className="space-y-1.5"><Amount planck={channel.claimed} decimals={6} /><Progress value={used(channel)} /></div>
            </Field>
            <Field label={channel.status === "closed" ? "Refunded" : "Left"}><Amount planck={channel.status === "closed" ? channel.refunded : remaining.toString()} decimals={6} /></Field>
            {channel.closes_at != null && (
              <Field label="Closes at">
                <span className="inline-flex items-center gap-2"><BlockLink number={channel.closes_at} />{best != null && channel.closes_at > best && <span className="text-sm text-muted">in about {blocksDuration(channel.closes_at - best)}</span>}</span>
              </Field>
            )}
            <Field label="Opened"><BlockLink number={channel.opened_block} /></Field>
            {channel.closed_block != null && <Field label="Closed"><BlockLink number={channel.closed_block} /></Field>}
          </Fields>
        </Card>
      </div>
    </Page>
  );
}
