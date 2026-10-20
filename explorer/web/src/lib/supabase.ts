import { createClient, type SupabaseClient } from "@supabase/supabase-js";

const url = import.meta.env.VITE_SUPABASE_URL as string | undefined;
const anonKey = import.meta.env.VITE_SUPABASE_ANON_KEY as string | undefined;

/** Whether the build has the Supabase project of the explorer. */
export const configured = Boolean(url && anonKey);

/** Read-only client: the anon key only reads, through row level security. */
export const supabase: SupabaseClient = createClient(url ?? "https://example.invalid", anonKey ?? "missing", {
  auth: { persistSession: false, autoRefreshToken: false },
  realtime: { params: { eventsPerSecond: 5 } },
});

export const network = (import.meta.env.VITE_NETWORK as string | undefined) ?? "Testnet";
