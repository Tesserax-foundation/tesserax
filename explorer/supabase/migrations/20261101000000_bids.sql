-- Bids of the market (runtime 106): a buyer bids for TESS, and sellers sell into the bid at once
-- (`docs/swaps.md` of the chain, "Bids: selling at once"). A claimed sale is a trade where the
-- taker, the seller, sold TESS; a claimed trade of an offer one where the taker bought TESS.

-- Open bids, read from the chain state by the indexer whenever a block has swap events.
create table public.swap_bids (
  id bigint primary key,
  buyer text not null,
  chain text not null,
  asset text not null,
  -- Smallest units of the asset for one TESS.
  price numeric(39, 0) not null,
  wanted numeric(39, 0) not null,
  min_sale numeric(39, 0) not null,
  sales integer not null,
  completed integer not null,
  created_block bigint not null,
  updated_block bigint not null
);
create index swap_bids_pair on public.swap_bids (chain, asset, price desc);
create index swap_bids_buyer on public.swap_bids (buyer);

alter table public.swap_bids enable row level security;
create policy "public read" on public.swap_bids for select to anon, authenticated using (true);

grant select, insert, update, delete on public.swap_bids to explorer_indexer;
create policy "indexer writes" on public.swap_bids for all to explorer_indexer using (true) with check (true);

create index events_swap_sale on public.events (((data->>'sale')::bigint)) where pallet = 'Swaps';

-- Sales: made into a bid by their seller, paid by the buyer on the other chain, then claimed for
-- the buyer or refunded to the seller once expired. The payment is the one the chain computes,
-- rounded up to the smallest unit.
create or replace view public.swap_sales with (security_invoker = true) as
select
  (s.data->>'sale')::bigint as sale,
  (s.data->>'bid')::bigint as bid,
  s.data->>'seller' as seller,
  s.data->>'buyer' as buyer,
  s.data->'payment'->>'chain' as chain,
  s.data->'payment'->>'asset' as asset,
  swap_symbol(s.data->'payment'->>'chain', s.data->'payment'->>'asset') as symbol,
  (s.data->>'amount')::numeric as amount,
  (s.data->>'price')::numeric as price,
  ceil((s.data->>'amount')::numeric * (s.data->>'price')::numeric / 1000000000000) as payment,
  s.data->>'hash' as hash,
  s.data->>'payout' as payout,
  (s.data->>'expires')::bigint as expires,
  s.block_number as sold_block,
  s.timestamp as sold_at,
  p.data->>'payment_lock' as payment_lock,
  p.block_number as paid_block,
  c.data->>'secret' as secret,
  c.block_number as claimed_block,
  c.timestamp as claimed_at,
  r.block_number as refunded_block,
  case
    when c.block_number is not null then 'claimed'
    when r.block_number is not null then 'refunded'
    when p.block_number is not null then 'paid'
    else 'sold'
  end as status
from events s
left join events p on p.pallet = 'Swaps' and p.name = 'SalePaid' and (p.data->>'sale')::bigint = (s.data->>'sale')::bigint
left join events c on c.pallet = 'Swaps' and c.name = 'SaleClaimed' and (c.data->>'sale')::bigint = (s.data->>'sale')::bigint
left join events r on r.pallet = 'Swaps' and r.name = 'SaleRefunded' and (r.data->>'sale')::bigint = (s.data->>'sale')::bigint
where s.pallet = 'Swaps' and s.name = 'Sold';

-- Completed trades of both kinds: trades of offers, where the taker bought TESS, and sales into
-- bids, where it sold them. Sales take the ids from 10^15 on (10^15 + the sale), which trades
-- never reach: an id names one trade of either kind.
create or replace view public.swap_completed with (security_invoker = true) as
select trade as trade_id, 'buy' as kind, chain, asset, symbol, amount, price, payment, claimed_block, claimed_at
from swap_trades where status = 'claimed'
union all
select 1000000000000000 + sale, 'sell', chain, asset, symbol, amount, price, payment, claimed_block, claimed_at
from swap_sales where status = 'claimed';

-- Pairs: TESS against every asset with an open offer or bid, or a trade.
create or replace function public.market_pairs()
returns jsonb
language sql stable
set search_path = public
as $$
  select coalesce(jsonb_agg(jsonb_build_object('ticker_id', 'TESS_' || symbol, 'base', 'TESS', 'target', symbol) order by symbol), '[]'::jsonb)
  from (
    select distinct swap_symbol(chain, asset) as symbol from swap_offers where available > 0
    union
    select distinct swap_symbol(chain, asset) from swap_bids where wanted > 0
    union
    select distinct symbol from swap_completed
  ) pairs;
$$;

-- Tickers over the last 24 hours, from the trades claimed and the sales claimed; the bid is the
-- best open bid, the ask the best open offer.
create or replace function public.market_tickers()
returns jsonb
language sql stable
set search_path = public
as $$
  with done as (select * from swap_completed),
  pairs as (
    select distinct swap_symbol(chain, asset) as symbol, swap_decimals(chain, asset) as decimals from swap_offers where available > 0
    union
    select distinct swap_symbol(chain, asset), swap_decimals(chain, asset) from swap_bids where wanted > 0
    union
    select distinct symbol, swap_decimals(chain, asset) from done
  ),
  day as (select * from done where claimed_at > now() - interval '24 hours')
  select coalesce(jsonb_agg(jsonb_build_object(
    'ticker_id', 'TESS_' || p.symbol,
    'base_currency', 'TESS',
    'target_currency', p.symbol,
    'last_price', (select swap_decimal(price, p.decimals) from done where symbol = p.symbol order by claimed_block desc, trade_id desc limit 1),
    'base_volume', (select swap_decimal(coalesce(sum(amount), 0), 12) from day where symbol = p.symbol),
    'target_volume', (select swap_decimal(coalesce(sum(payment), 0), p.decimals) from day where symbol = p.symbol),
    'bid', coalesce((select swap_decimal(max(price), p.decimals) from swap_bids where swap_symbol(chain, asset) = p.symbol and wanted > 0), '0'),
    'ask', (select swap_decimal(min(price), p.decimals) from swap_offers where swap_symbol(chain, asset) = p.symbol and available > 0),
    'high', (select swap_decimal(max(price), p.decimals) from day where symbol = p.symbol),
    'low', (select swap_decimal(min(price), p.decimals) from day where symbol = p.symbol)
  ) order by p.symbol), '[]'::jsonb)
  from (select distinct symbol, decimals from pairs) p;
$$;

-- Order book of `ticker_id`: the open bids, the highest price first, and the open offers, the
-- lowest first. `depth` counts the levels of both sides, half on each, as CoinGecko asks.
create or replace function public.market_orderbook(ticker_id text, depth integer default 100)
returns jsonb
language sql stable
set search_path = public
as $$
  with side as (select greatest(least(coalesce(depth, 100), 500) / 2, 1) as levels),
  bids as (
    select price, sum(wanted) as wanted, swap_decimals(chain, asset) as decimals
    from swap_bids
    where 'TESS_' || swap_symbol(chain, asset) = ticker_id and wanted > 0
    group by price, swap_decimals(chain, asset)
    order by price desc
    limit (select levels from side)
  ),
  asks as (
    select price, sum(available) as available, swap_decimals(chain, asset) as decimals
    from swap_offers
    where 'TESS_' || swap_symbol(chain, asset) = ticker_id and available > 0
    group by price, swap_decimals(chain, asset)
    order by price
    limit (select levels from side)
  )
  select jsonb_build_object(
    'ticker_id', ticker_id,
    'timestamp', (extract(epoch from now()) * 1000)::bigint::text,
    'bids', coalesce((select jsonb_agg(jsonb_build_array(swap_decimal(price, decimals), swap_decimal(wanted, 12)) order by price desc) from bids), '[]'::jsonb),
    'asks', coalesce((select jsonb_agg(jsonb_build_array(swap_decimal(price, decimals), swap_decimal(available, 12)) order by price) from asks), '[]'::jsonb)
  );
$$;

-- Trades of `ticker_id`, the most recent first: claimed trades of offers are buys, claimed sales
-- into bids are sells.
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
    select * from swap_completed d
    where 'TESS_' || d.symbol = ticker_id
      and (start_time is null or d.claimed_at >= start_time)
      and (end_time is null or d.claimed_at <= end_time)
      and (market_trades.kind is null or d.kind = market_trades.kind)
    order by d.claimed_block desc, d.trade_id desc
    limit greatest(least(coalesce(max_rows, 500), 1000), 1)
  ),
  rows as (
    select kind, claimed_block, trade_id, jsonb_build_object(
      'trade_id', trade_id,
      'price', swap_decimal(price, swap_decimals(chain, asset)),
      'base_volume', swap_decimal(amount, 12),
      'target_volume', swap_decimal(payment, swap_decimals(chain, asset)),
      'trade_timestamp', (extract(epoch from claimed_at) * 1000)::bigint,
      'type', kind
    ) as trade
    from chosen
  )
  select jsonb_build_object(
    'buy', coalesce((select jsonb_agg(trade order by claimed_block desc, trade_id desc) from rows where kind = 'buy'), '[]'::jsonb),
    'sell', coalesce((select jsonb_agg(trade order by claimed_block desc, trade_id desc) from rows where kind = 'sell'), '[]'::jsonb)
  );
$$;
