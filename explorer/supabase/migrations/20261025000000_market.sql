-- The market of TESS: atomic swaps against BTC, ETH, USDC and USDT (`docs/swaps.md` of the chain),
-- and the data price aggregators such as CoinGecko read, in their formats. Prices are in the
-- asset paid for one TESS; volumes in TESS (base) and in the asset (target).

-- Open offers, read from the chain state by the indexer whenever a block has swap events.
create table public.swap_offers (
  id bigint primary key,
  maker text not null,
  chain text not null,
  asset text not null,
  -- Smallest units of the asset for one TESS.
  price numeric(39, 0) not null,
  available numeric(39, 0) not null,
  min_fill numeric(39, 0) not null,
  payout text not null,
  fills integer not null,
  completed integer not null,
  updated_block bigint not null
);
create index swap_offers_pair on public.swap_offers (chain, asset, price);

alter table public.swap_offers enable row level security;
create policy "public read" on public.swap_offers for select to anon, authenticated using (true);

create index events_swap_trade on public.events (((data->>'trade')::bigint)) where pallet = 'Swaps';

-- Symbol and decimals of the asset of a payment.
create or replace function public.swap_symbol(chain text, asset text)
returns text
language sql immutable
as $$
  select case when chain = 'Bitcoin' then 'BTC' when asset = 'Native' then 'ETH' when asset = 'Usdc' then 'USDC' else 'USDT' end;
$$;

create or replace function public.swap_decimals(chain text, asset text)
returns integer
language sql immutable
as $$
  select case when chain = 'Bitcoin' then 8 when asset = 'Native' then 18 else 6 end;
$$;

-- `value` smallest units of an asset of `decimals` as a decimal number, without trailing zeros.
create or replace function public.swap_decimal(value numeric, decimals integer)
returns text
language sql immutable
as $$
  select trim_scale(round(value / (10::numeric ^ decimals), decimals))::text;
$$;

-- Trades: filled by their maker, then claimed by their taker or refunded once expired. The
-- payment is the one the chain computes, rounded up to the smallest unit.
create or replace view public.swap_trades with (security_invoker = true) as
select
  (f.data->>'trade')::bigint as trade,
  (f.data->>'offer')::bigint as offer,
  f.data->>'maker' as maker,
  f.data->>'taker' as taker,
  f.data->'payment'->>'chain' as chain,
  f.data->'payment'->>'asset' as asset,
  swap_symbol(f.data->'payment'->>'chain', f.data->'payment'->>'asset') as symbol,
  (f.data->>'amount')::numeric as amount,
  (f.data->>'price')::numeric as price,
  ceil((f.data->>'amount')::numeric * (f.data->>'price')::numeric / 1000000000000) as payment,
  f.block_number as filled_block,
  f.timestamp as filled_at,
  c.block_number as claimed_block,
  c.timestamp as claimed_at,
  r.block_number as refunded_block,
  case when c.block_number is not null then 'claimed' when r.block_number is not null then 'refunded' else 'filled' end as status
from events f
left join events c on c.pallet = 'Swaps' and c.name = 'Claimed' and (c.data->>'trade')::bigint = (f.data->>'trade')::bigint
left join events r on r.pallet = 'Swaps' and r.name = 'Refunded' and (r.data->>'trade')::bigint = (f.data->>'trade')::bigint
where f.pallet = 'Swaps' and f.name = 'Filled';

-- Pairs: TESS against every asset with an open offer or a trade.
create or replace function public.market_pairs()
returns jsonb
language sql stable
set search_path = public
as $$
  select coalesce(jsonb_agg(jsonb_build_object('ticker_id', 'TESS_' || symbol, 'base', 'TESS', 'target', symbol) order by symbol), '[]'::jsonb)
  from (
    select distinct swap_symbol(chain, asset) as symbol from swap_offers where available > 0
    union
    select distinct symbol from swap_trades where status = 'claimed'
  ) pairs;
$$;

-- Tickers over the last 24 hours, from the trades their taker claimed; the ask is the best open
-- offer. Offers sell TESS only: there is no bid.
create or replace function public.market_tickers()
returns jsonb
language sql stable
set search_path = public
as $$
  with pairs as (
    select distinct swap_symbol(chain, asset) as symbol, swap_decimals(chain, asset) as decimals from swap_offers where available > 0
    union
    select distinct symbol, swap_decimals(chain, asset) from swap_trades where status = 'claimed'
  ),
  done as (select * from swap_trades where status = 'claimed'),
  day as (select * from done where claimed_at > now() - interval '24 hours')
  select coalesce(jsonb_agg(jsonb_build_object(
    'ticker_id', 'TESS_' || p.symbol,
    'base_currency', 'TESS',
    'target_currency', p.symbol,
    'last_price', (select swap_decimal(price, p.decimals) from done where symbol = p.symbol order by claimed_block desc, trade desc limit 1),
    'base_volume', (select swap_decimal(coalesce(sum(amount), 0), 12) from day where symbol = p.symbol),
    'target_volume', (select swap_decimal(coalesce(sum(payment), 0), p.decimals) from day where symbol = p.symbol),
    'bid', '0',
    'ask', (select swap_decimal(min(price), p.decimals) from swap_offers where swap_symbol(chain, asset) = p.symbol and available > 0),
    'high', (select swap_decimal(max(price), p.decimals) from day where symbol = p.symbol),
    'low', (select swap_decimal(min(price), p.decimals) from day where symbol = p.symbol)
  ) order by p.symbol), '[]'::jsonb)
  from (select distinct symbol, decimals from pairs) p;
$$;

-- Order book of `ticker_id`: the open offers by price, `depth` levels at most.
create or replace function public.market_orderbook(ticker_id text, depth integer default 100)
returns jsonb
language sql stable
set search_path = public
as $$
  with asks as (
    select price, sum(available) as available, swap_decimals(chain, asset) as decimals
    from swap_offers
    where 'TESS_' || swap_symbol(chain, asset) = ticker_id and available > 0
    group by price, swap_decimals(chain, asset)
    order by price
    limit greatest(least(coalesce(depth, 100), 500), 1)
  )
  select jsonb_build_object(
    'ticker_id', ticker_id,
    'timestamp', (extract(epoch from now()) * 1000)::bigint::text,
    'bids', '[]'::jsonb,
    'asks', coalesce((select jsonb_agg(jsonb_build_array(swap_decimal(price, decimals), swap_decimal(available, 12)) order by price) from asks), '[]'::jsonb)
  );
$$;

-- Trades of `ticker_id` its taker claimed, the most recent first. Takers buy TESS: every trade is
-- a buy.
create or replace function public.market_trades(
  ticker_id text,
  kind text default null,
  max_rows integer default 500,
  start_time timestamptz default null,
  end_time timestamptz default null
)
returns jsonb
language sql stable
set search_path = public
as $$
  with chosen as (
    select * from swap_trades
    where status = 'claimed' and 'TESS_' || symbol = ticker_id
      and (start_time is null or claimed_at >= start_time)
      and (end_time is null or claimed_at <= end_time)
      and (kind is null or kind = 'buy')
    order by claimed_block desc, trade desc
    limit greatest(least(coalesce(max_rows, 500), 1000), 1)
  )
  select jsonb_build_object(
    'buy', coalesce((select jsonb_agg(jsonb_build_object(
      'trade_id', trade,
      'price', swap_decimal(price, swap_decimals(chain, asset)),
      'base_volume', swap_decimal(amount, 12),
      'target_volume', swap_decimal(payment, swap_decimals(chain, asset)),
      'trade_timestamp', (extract(epoch from claimed_at) * 1000)::bigint,
      'type', 'buy'
    ) order by claimed_block desc, trade desc) from chosen), '[]'::jsonb),
    'sell', '[]'::jsonb
  );
$$;

-- Supply of TESS: the total issuance, what circulates (the issuance without what the genesis
-- allocations still vest at the best block) and the maximum (mining supply and allocations).
create or replace function public.market_supply()
returns jsonb
language sql stable
set search_path = public
as $$
  with head as (select (value->>'best')::numeric as best from chain_state where key = 'head'),
  supply as (select value from chain_state where key = 'supply'),
  allocations as (select coalesce(value, '[]'::jsonb) as value from chain_state where key = 'allocations'),
  vesting as (
    select coalesce(sum(greatest((s->>'locked')::numeric - (s->>'per_block')::numeric * greatest((select best from head) - (s->>'starting_block')::numeric, 0), 0)), 0) as locked
    from allocations, jsonb_array_elements(allocations.value) a, jsonb_array_elements(a->'vesting') s
  ),
  allocated as (
    select coalesce(sum((a->>'amount')::numeric), 0) as amount from allocations, jsonb_array_elements(allocations.value) a
  )
  select jsonb_build_object(
    'total', swap_decimal((select (value->>'total_issuance')::numeric from supply), 12),
    'circulating', swap_decimal((select (value->>'total_issuance')::numeric from supply) - (select locked from vesting), 12),
    'max', swap_decimal((select (value->>'mining_supply')::numeric from supply) + (select amount from allocated), 12)
  );
$$;

grant execute on function public.market_pairs(), public.market_tickers(), public.market_orderbook(text, integer),
  public.market_trades(text, text, integer, timestamptz, timestamptz), public.market_supply() to anon, authenticated;
