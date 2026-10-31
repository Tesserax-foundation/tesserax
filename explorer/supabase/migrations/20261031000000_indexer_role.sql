-- The indexer writes through a role of its own, limited to the rows of the explorer's tables: it
-- cannot change the schema, read other schemas or touch the authentication of the project.
--
-- The role logs in with a password set outside the migrations, in supabase/roles.sql (git-ignored,
-- pushed with `supabase db push --include-roles`). Its connection string, through the session
-- pooler: postgresql://explorer_indexer.<project ref>:<password>@<pooler host>:5432/postgres

do $$
begin
  if not exists (select from pg_roles where rolname = 'explorer_indexer') then
    create role explorer_indexer login noinherit;
  end if;
end
$$;

grant usage on schema public to explorer_indexer;

do $$
declare
  t text;
begin
  foreach t in array array[
    'blocks', 'extrinsics', 'events', 'transfers', 'accounts', 'agents', 'delegations',
    'channels', 'tasks', 'chain_state', 'shielded_transactions', 'swap_offers'
  ] loop
    execute format('grant select, insert, update, delete on public.%I to explorer_indexer', t);
    -- Row level security only lets the public read: the indexer gets a policy of its own.
    execute format(
      'create policy "indexer writes" on public.%I for all to explorer_indexer using (true) with check (true)',
      t
    );
  end loop;
end
$$;
