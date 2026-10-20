-- Tesserax Explorer: the chain as indexed by `explorer/indexer`.
--
-- The indexer writes with the database owner; the web application reads with the anon key,
-- which row level security limits to SELECT. Amounts are in planck (10^-12 TESS) and returned
-- as text by the functions below, as JSON numbers would lose precision.

-- Blocks, including the blocks a reorganization removed from the best chain (canonical = false).
create table public.blocks (
  hash text primary key,
  number bigint not null,
  parent_hash text not null,
  state_root text not null,
  extrinsics_root text not null,
  author text,
  timestamp timestamptz,
  difficulty numeric(78, 0),
  pow_nonce text,
  pow_hash text,
  extrinsic_count integer not null default 0,
  event_count integer not null default 0,
  transfer_count integer not null default 0,
  reward numeric(39, 0) not null default 0,
  fees numeric(39, 0) not null default 0,
  size integer not null,
  spec_version integer not null,
  canonical boolean not null default true,
  finalized boolean not null default false,
  indexed_at timestamptz not null default now()
);
create unique index blocks_canonical_number on public.blocks (number) where canonical;
create index blocks_number on public.blocks (number desc);
create index blocks_author on public.blocks (author, number desc) where canonical;
create index blocks_timestamp on public.blocks (timestamp desc) where canonical;

-- Extrinsics of canonical blocks. Inherents have no signer.
create table public.extrinsics (
  block_hash text not null references public.blocks (hash) on delete cascade,
  block_number bigint not null,
  index integer not null,
  hash text not null,
  signer text,
  pallet text not null,
  call text not null,
  args jsonb not null,
  nonce bigint,
  tip numeric(39, 0),
  fee numeric(39, 0),
  success boolean not null,
  error text,
  size integer not null,
  timestamp timestamptz,
  primary key (block_hash, index)
);
create index extrinsics_hash on public.extrinsics (hash);
create index extrinsics_signer on public.extrinsics (signer, block_number desc, index desc);
create index extrinsics_signed on public.extrinsics (block_number desc, index desc) where signer is not null;
create index extrinsics_call on public.extrinsics (pallet, call, block_number desc);

-- Events of canonical blocks.
create table public.events (
  block_hash text not null references public.blocks (hash) on delete cascade,
  block_number bigint not null,
  index integer not null,
  extrinsic_index integer,
  pallet text not null,
  name text not null,
  data jsonb not null,
  timestamp timestamptz,
  primary key (block_hash, index)
);
create index events_name on public.events (pallet, name, block_number desc);
create index events_extrinsic on public.events (block_hash, extrinsic_index);
create index events_recent on public.events (block_number desc, index desc);

-- Balance transfers (Balances.Transfer events) of canonical blocks.
create table public.transfers (
  block_hash text not null references public.blocks (hash) on delete cascade,
  block_number bigint not null,
  event_index integer not null,
  extrinsic_index integer,
  extrinsic_hash text,
  from_address text not null,
  to_address text not null,
  amount numeric(39, 0) not null,
  timestamp timestamptz,
  primary key (block_hash, event_index)
);
create index transfers_from on public.transfers (from_address, block_number desc);
create index transfers_to on public.transfers (to_address, block_number desc);
create index transfers_recent on public.transfers (block_number desc, event_index desc);

-- Accounts, as of the latest block that touched them.
create table public.accounts (
  address text primary key,
  free numeric(39, 0) not null default 0,
  reserved numeric(39, 0) not null default 0,
  frozen numeric(39, 0) not null default 0,
  nonce bigint not null default 0,
  first_seen_block bigint,
  updated_block bigint not null
);
create index accounts_total on public.accounts ((free + reserved) desc);

-- Agent registry (pallet AgentRegistry).
create table public.agents (
  address text primary key,
  owner text,
  owner_confirmed boolean not null default false,
  name text not null default '',
  description text not null default '',
  endpoint text not null default '',
  capabilities text[] not null default '{}',
  metadata text,
  active boolean not null default true,
  registered_at bigint,
  deposit numeric(39, 0) not null default 0,
  payments bigint not null default 0,
  volume numeric(39, 0) not null default 0,
  payers bigint not null default 0,
  positive bigint not null default 0,
  negative bigint not null default 0,
  tasks_completed bigint not null default 0,
  tasks_failed bigint not null default 0,
  deregistered boolean not null default false,
  updated_block bigint not null
);
create index agents_capabilities on public.agents using gin (capabilities);
create index agents_volume on public.agents (volume desc);
create index agents_owner on public.agents (owner);

-- Delegations from owners to agent keys (pallet Agents).
create table public.delegations (
  owner text not null,
  agent text not null,
  budget numeric(39, 0) not null,
  period bigint not null,
  max_per_call numeric(39, 0) not null,
  expires_at bigint,
  transfers boolean not null,
  contracts boolean not null,
  targets text[] not null default '{}',
  spent numeric(39, 0) not null default 0,
  period_start bigint not null,
  active boolean not null default true,
  updated_block bigint not null,
  primary key (owner, agent)
);
create index delegations_agent on public.delegations (agent);

-- Payment channels (pallet Channels).
create table public.channels (
  id bigint primary key,
  payer text not null,
  payee text not null,
  signer text not null,
  deposit numeric(39, 0) not null,
  claimed numeric(39, 0) not null default 0,
  closes_at bigint,
  status text not null check (status in ('open', 'closing', 'closed')),
  refunded numeric(39, 0),
  opened_block bigint,
  closed_block bigint,
  updated_block bigint not null
);
create index channels_payer on public.channels (payer);
create index channels_payee on public.channels (payee);

-- Task market (pallet Tasks).
create table public.tasks (
  id bigint primary key,
  client text not null,
  spec text not null,
  capability text,
  reward numeric(39, 0) not null,
  invited text,
  arbiter text,
  arbiter_fee numeric(39, 0),
  bond numeric(39, 0) not null default 0,
  delivery_period bigint not null,
  review_period bigint not null,
  status text not null check (status in ('open', 'assigned', 'delivered', 'disputed', 'completed', 'cancelled', 'expired')),
  worker text,
  deadline bigint,
  result text,
  review_until bigint,
  paid numeric(39, 0),
  refunded numeric(39, 0),
  created_block bigint,
  updated_block bigint not null
);
create index tasks_status on public.tasks (status, id desc);
create index tasks_client on public.tasks (client);
create index tasks_worker on public.tasks (worker);
create index tasks_capability on public.tasks (capability);

-- Chain-wide values maintained by the indexer: head, issuance, emission, shielded pool,
-- constants. One row per key.
create table public.chain_state (
  key text primary key,
  value jsonb not null,
  updated_at timestamptz not null default now()
);

-- Read access for everyone, writes only by the indexer (database owner).
do $$
declare t text;
begin
  foreach t in array array['blocks', 'extrinsics', 'events', 'transfers', 'accounts', 'agents',
                           'delegations', 'channels', 'tasks', 'chain_state'] loop
    execute format('alter table public.%I enable row level security', t);
    execute format('create policy "public read" on public.%I for select to anon, authenticated using (true)', t);
  end loop;
end $$;

-- New blocks and head updates are pushed to the web application.
alter publication supabase_realtime add table public.blocks, public.chain_state;

-- Overview of the network for the home page.
create or replace function public.explorer_overview()
returns jsonb
language sql stable
set search_path = public
as $$
  with recent as (
    select number, timestamp, difficulty from blocks
    where canonical and timestamp is not null
    order by number desc limit 121
  ),
  timing as (
    select
      extract(epoch from max(timestamp) - min(timestamp)) / nullif(count(*) - 1, 0) as block_time,
      avg(difficulty) as difficulty
    from recent
  )
  select jsonb_build_object(
    'best', (select max(number) from blocks where canonical),
    'finalized', (select max(number) from blocks where canonical and finalized),
    'block_time', (select round(block_time::numeric, 2) from timing),
    'hashrate', (select round(difficulty / nullif(block_time, 0))::text from timing),
    'difficulty', (select difficulty::text from blocks where canonical order by number desc limit 1),
    'extrinsics_24h', (select count(*) from extrinsics where signer is not null and timestamp > now() - interval '24 hours'),
    'transfers_24h', (select count(*) from transfers where timestamp > now() - interval '24 hours'),
    'volume_24h', (select coalesce(sum(amount), 0)::text from transfers where timestamp > now() - interval '24 hours'),
    'miners_24h', (select count(distinct author) from blocks where canonical and timestamp > now() - interval '24 hours'),
    'accounts', (select count(*) from accounts),
    'agents', (select count(*) from agents where not deregistered),
    'open_tasks', (select count(*) from tasks where status in ('open', 'assigned', 'delivered', 'disputed')),
    'open_channels', (select count(*) from channels where status <> 'closed'),
    'orphaned_24h', (select count(*) from blocks where not canonical and timestamp > now() - interval '24 hours')
  );
$$;

-- Activity per day over the last `days` days.
create or replace function public.daily_stats(days integer default 30)
returns table (
  day date,
  blocks bigint,
  extrinsics bigint,
  transfers bigint,
  volume text,
  avg_block_time numeric,
  avg_difficulty text,
  miners bigint,
  rewards text,
  fees text
)
language sql stable
set search_path = public
as $$
  with b as (
    select timestamp::date as day, count(*) as blocks,
           extract(epoch from max(timestamp) - min(timestamp)) / nullif(count(*) - 1, 0) as block_time,
           avg(difficulty) as difficulty, count(distinct author) as miners,
           sum(reward) as rewards, sum(fees) as fees
    from blocks
    where canonical and timestamp > now() - make_interval(days => days)
    group by 1
  ),
  x as (
    select timestamp::date as day, count(*) as extrinsics from extrinsics
    where signer is not null and timestamp > now() - make_interval(days => days) group by 1
  ),
  t as (
    select timestamp::date as day, count(*) as transfers, sum(amount) as volume from transfers
    where timestamp > now() - make_interval(days => days) group by 1
  )
  select b.day, b.blocks, coalesce(x.extrinsics, 0), coalesce(t.transfers, 0),
         coalesce(t.volume, 0)::text, round(b.block_time::numeric, 2), round(b.difficulty)::text,
         b.miners, b.rewards::text, b.fees::text
  from b left join x using (day) left join t using (day)
  order by b.day;
$$;

-- Miners ranked by blocks found since `since`.
create or replace function public.top_miners(since interval default interval '7 days', max_rows integer default 50)
returns table (author text, blocks bigint, rewards text, fees text, share numeric, last_block bigint)
language sql stable
set search_path = public
as $$
  with m as (
    select author, count(*) as blocks, sum(reward) as rewards, sum(fees) as fees, max(number) as last_block
    from blocks
    where canonical and author is not null and timestamp > now() - since
    group by author
  )
  select author, blocks, rewards::text, fees::text,
         round(100.0 * blocks / nullif(sum(blocks) over (), 0), 2), last_block
  from m order by blocks desc limit max_rows;
$$;

-- Accounts ranked by total balance.
create or replace function public.rich_list(max_rows integer default 100, skip integer default 0)
returns table (address text, free text, reserved text, frozen text, total text, nonce bigint, share numeric)
language sql stable
set search_path = public
as $$
  select a.address, a.free::text, a.reserved::text, a.frozen::text, (a.free + a.reserved)::text, a.nonce,
         round(100.0 * (a.free + a.reserved) / nullif((select (value->>'total_issuance')::numeric
                                                          from chain_state where key = 'supply'), 0), 4)
  from accounts a
  order by a.free + a.reserved desc
  limit max_rows offset skip;
$$;

-- What a search box query designates: a block number or hash, an extrinsic hash, an address,
-- an agent name, a task or a channel id.
create or replace function public.explorer_search(query text)
returns table (kind text, id text, label text)
language sql stable
set search_path = public
as $$
  with q as (select trim(query) as q)
  select 'block', number::text, 'Block #' || number from blocks, q
    where canonical and q ~ '^[0-9]+$' and number = q::bigint
  union all
  select 'block', hash, 'Block #' || number from blocks, q where lower(q) = hash
  union all
  select 'extrinsic', hash, 'Extrinsic ' || block_number || '-' || index || ' ' || pallet || '.' || call
    from extrinsics, q where lower(q) = hash
  union all
  select 'account', address, address from accounts, q where q = address
  union all
  select 'agent', address, name from agents, q
    where length(q) >= 2 and name ilike '%' || replace(replace(q, '%', ''), '_', '') || '%'
  union all
  select 'task', id::text, 'Task #' || id from tasks, q where q ~ '^#?[0-9]+$' and id = ltrim(q, '#')::bigint
  union all
  select 'channel', id::text, 'Channel #' || id from channels, q where q ~ '^#?[0-9]+$' and id = ltrim(q, '#')::bigint
  limit 20;
$$;

grant execute on function public.explorer_overview(), public.daily_stats(integer),
  public.top_miners(interval, integer), public.rich_list(integer, integer),
  public.explorer_search(text) to anon, authenticated;
