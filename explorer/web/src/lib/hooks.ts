import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { supabase } from "./supabase";

/** The current time, refreshed every `interval` ms, for relative times. */
export function useNow(interval = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), interval);
    return () => clearInterval(timer);
  }, [interval]);
  return now;
}

/** New blocks and head updates refresh the views that show them. */
export function useRealtime(onBlock?: (number: number) => void): void {
  const client = useQueryClient();
  useEffect(() => {
    const channel = supabase
      .channel("explorer")
      .on("postgres_changes", { event: "INSERT", schema: "public", table: "blocks" }, (payload) => {
        const number = (payload.new as { number?: number }).number;
        for (const key of ["blocks", "extrinsics", "transfers", "overview", "events"]) client.invalidateQueries({ queryKey: [key] });
        if (number != null) onBlock?.(number);
      })
      .on("postgres_changes", { event: "*", schema: "public", table: "chain_state" }, () => {
        client.invalidateQueries({ queryKey: ["chain-state"] });
      })
      .subscribe();
    return () => {
      void supabase.removeChannel(channel);
    };
  }, [client, onBlock]);
}

type Theme = "dark" | "light";

export function useTheme(): [Theme, () => void] {
  const [theme, setTheme] = useState<Theme>(() => (document.documentElement.classList.contains("dark") ? "dark" : "light"));
  const toggle = () => {
    const next: Theme = theme === "dark" ? "light" : "dark";
    document.documentElement.classList.toggle("dark", next === "dark");
    try {
      localStorage.setItem("theme", next);
    } catch {
      // Private browsing: the theme lasts for the page only.
    }
    setTheme(next);
  };
  return [theme, toggle];
}

/** Page index from the `page` search parameter (1-based in the URL). */
export function pageIndex(params: URLSearchParams): number {
  const page = Number(params.get("page") ?? "1");
  return Number.isInteger(page) && page > 0 ? page - 1 : 0;
}
