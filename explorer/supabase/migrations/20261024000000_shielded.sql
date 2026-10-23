-- Transactions of the shielded pool, for wallets to read every note of the tree in one request:
-- only public data, the notes encrypted for their recipients. A wallet checks the tree it builds
-- from them against the roots on chain. Digests are the 32 bytes of their SCALE encoding.
create table public.shielded_transactions (
  block_hash text not null references public.blocks (hash) on delete cascade,
  block_number bigint not null,
  event_index integer not null,
  -- Position of the first of the two commitments in the note tree.
  position bigint not null,
  nullifiers text[] not null,
  commitments text[] not null,
  notes text[] not null,
  value_in numeric(39, 0) not null,
  value_out numeric(39, 0) not null,
  fee numeric(39, 0) not null,
  primary key (block_hash, event_index)
);
create index shielded_transactions_position on public.shielded_transactions (position);

alter table public.shielded_transactions enable row level security;
create policy "public read" on public.shielded_transactions for select to anon, authenticated using (true);
