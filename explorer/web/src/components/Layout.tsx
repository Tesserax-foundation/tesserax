import { Blocks, Bot, ChevronDown, Coins, Cpu, Menu, Moon, Network, Pickaxe, ScrollText, Shield, Sun, Users, Wallet, X, Zap } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Link, Outlet, ScrollRestoration, useLocation } from "react-router";
import { useChainState } from "../lib/api";
import { timeAgo } from "../lib/format";
import { useNow, useRealtime, useTheme } from "../lib/hooks";
import { network } from "../lib/supabase";
import { SearchBar } from "./Search";
import { cx } from "./ui";

interface Item {
  to: string;
  label: string;
  icon: ReactNode;
  description?: string;
}

const groups: Array<{ label: string; items: Item[] }> = [
  {
    label: "Blockchain",
    items: [
      { to: "/blocks", label: "Blocks", icon: <Blocks size={16} />, description: "Latest blocks and their miners" },
      { to: "/extrinsics", label: "Transactions", icon: <ScrollText size={16} />, description: "Signed transactions" },
      { to: "/transfers", label: "Transfers", icon: <Coins size={16} />, description: "TESS moving between accounts" },
      { to: "/events", label: "Events", icon: <Zap size={16} />, description: "Everything the runtime emitted" },
      { to: "/blocks?orphaned=1", label: "Orphaned blocks", icon: <Network size={16} />, description: "Blocks reorganizations replaced" },
    ],
  },
  {
    label: "Network",
    items: [
      { to: "/miners", label: "Miners", icon: <Pickaxe size={16} />, description: "Who finds the blocks" },
      { to: "/accounts", label: "Top accounts", icon: <Wallet size={16} />, description: "Accounts by balance" },
      { to: "/supply", label: "Supply & emission", icon: <Cpu size={16} />, description: "Rewards, halvings, allocations" },
      { to: "/shielded", label: "Shielded pool", icon: <Shield size={16} />, description: "Private zk-STARK transfers" },
    ],
  },
  {
    label: "AI agents",
    items: [
      { to: "/agents", label: "Agent registry", icon: <Bot size={16} />, description: "Agents, capabilities, reputation" },
      { to: "/tasks", label: "Task market", icon: <Users size={16} />, description: "Work paid from escrow" },
      { to: "/channels", label: "Payment channels", icon: <Zap size={16} />, description: "Micropayments between agents" },
    ],
  },
];

function Logo() {
  return (
    <Link to="/" className="flex shrink-0 items-center gap-2.5" aria-label="Tesserax Explorer home">
      <img src="/logo.png" alt="" width={34} height={34} className="size-[34px] drop-shadow-[0_0_10px_rgba(77,134,255,0.35)]" />
      <span className="leading-tight">
        <span className="block text-[17px] font-bold tracking-tight">Tesserax</span>
        <span className="block text-[11px] font-medium uppercase tracking-[0.18em] text-muted">Explorer</span>
      </span>
    </Link>
  );
}

function Dropdown({ label, items }: { label: string; items: Item[] }) {
  const [open, setOpen] = useState(false);
  const location = useLocation();
  useEffect(() => setOpen(false), [location]);
  const active = items.some((item) => location.pathname.startsWith(item.to.split("?")[0]));
  return (
    <div className="relative" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <button
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className={cx("flex h-9 items-center gap-1 rounded-lg px-3 text-sm font-medium transition-colors", active ? "text-fg" : "text-muted hover:text-fg")}
      >
        {label}
        <ChevronDown size={14} className={cx("transition-transform", open && "rotate-180")} />
      </button>
      {open && (
        <div className="absolute left-0 top-full z-50 w-72 pt-2">
          <div className="rounded-2xl border border-line bg-surface p-1.5 shadow-xl shadow-black/10">
            {items.map((item) => (
              <Link key={item.to} to={item.to} className="flex items-start gap-3 rounded-xl px-3 py-2.5 hover:bg-surface-2">
                <span className="mt-0.5 text-accent">{item.icon}</span>
                <span>
                  <span className="block text-sm font-medium">{item.label}</span>
                  {item.description && <span className="block text-xs text-muted">{item.description}</span>}
                </span>
              </Link>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function ThemeButton() {
  const [theme, toggle] = useTheme();
  return (
    <button onClick={toggle} aria-label={theme === "dark" ? "Light theme" : "Dark theme"} className="flex size-9 items-center justify-center rounded-lg border border-line bg-surface text-muted hover:text-fg">
      {theme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
    </button>
  );
}

function NetworkBadge() {
  return (
    <span className="hidden items-center gap-1.5 rounded-full border border-line bg-surface px-2.5 py-1 text-xs font-medium text-muted sm:inline-flex">
      <span className="size-1.5 rounded-full bg-success shadow-[0_0_6px_var(--success)]" />
      {network}
    </span>
  );
}

/** Whether `to` is the current page: "/blocks" and "/blocks?orphaned=1" are different pages. */
function isCurrent(to: string, location: { pathname: string; search: string }): boolean {
  const [path, query] = to.split("?");
  if (location.pathname !== path) return false;
  if (query) return location.search === `?${query}`;
  const others = groups.flatMap((g) => g.items).filter((i) => i.to.startsWith(`${path}?`));
  return !others.some((i) => location.search === `?${i.to.split("?")[1]}`);
}

function MobileMenu({ onClose }: { onClose: () => void }) {
  const location = useLocation();
  return (
    <div className="fixed inset-0 z-50 lg:hidden">
      <div className="absolute inset-0 bg-black/40 backdrop-blur-sm" onClick={onClose} />
      <nav className="absolute right-0 top-0 flex h-full w-[min(88vw,360px)] flex-col overflow-y-auto border-l border-line bg-surface p-4">
        <div className="mb-4 flex items-center justify-between">
          <Logo />
          <button onClick={onClose} aria-label="Close the menu" className="flex size-9 items-center justify-center rounded-lg border border-line">
            <X size={18} />
          </button>
        </div>
        <SearchBar className="mb-4" onDone={onClose} />
        {groups.map((group) => (
          <div key={group.label} className="mb-4">
            <div className="px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-faint">{group.label}</div>
            {group.items.map((item) => (
              <Link
                key={item.to}
                to={item.to}
                onClick={onClose}
                className={cx("flex items-center gap-3 rounded-xl px-2 py-2.5 text-sm font-medium", isCurrent(item.to, location) ? "bg-accent-soft text-accent" : "hover:bg-surface-2")}
              >
                <span className="text-accent">{item.icon}</span>
                {item.label}
              </Link>
            ))}
          </div>
        ))}
      </nav>
    </div>
  );
}

function IndexStatus() {
  const { data } = useChainState();
  const now = useNow(10_000);
  const head = data?.head;
  if (!head) return null;
  const behind = head.node_best - head.best;
  return (
    <span className="tabular">
      Indexed block #{head.best.toLocaleString("en-US")}
      {behind > 1 ? ` · node at #${head.node_best.toLocaleString("en-US")}` : ""} · updated {timeAgo(data?.updated_at, now)}
    </span>
  );
}

export function Layout() {
  const [menu, setMenu] = useState(false);
  const location = useLocation();
  useRealtime();
  useEffect(() => setMenu(false), [location.pathname]);
  const home = location.pathname === "/";
  return (
    <div className="flex min-h-dvh flex-col">
      <header className="sticky top-0 z-40 border-b border-line bg-bg/80 backdrop-blur-xl">
        <div className="mx-auto flex h-16 max-w-7xl items-center gap-4 px-4 sm:px-6">
          <Logo />
          <nav className="ml-4 hidden items-center lg:flex">
            {groups.map((group) => (
              <Dropdown key={group.label} label={group.label} items={group.items} />
            ))}
          </nav>
          <div className="ml-auto flex items-center gap-2">
            {!home && <SearchBar className="hidden w-72 md:block xl:w-80" />}
            <NetworkBadge />
            <ThemeButton />
            <button onClick={() => setMenu(true)} aria-label="Open the menu" className="flex size-9 items-center justify-center rounded-lg border border-line bg-surface lg:hidden">
              <Menu size={18} />
            </button>
          </div>
        </div>
      </header>
      {menu && <MobileMenu onClose={() => setMenu(false)} />}
      <main className="flex-1">
        <Outlet />
      </main>
      <footer className="border-t border-line">
        <div className="mx-auto flex max-w-7xl flex-col gap-3 px-4 py-6 text-sm text-muted sm:flex-row sm:items-center sm:justify-between sm:px-6">
          <div className="flex items-center gap-2">
            <img src="/logo.png" alt="" className="size-5" />
            <span>Tesserax Explorer · post-quantum proof of work for AI agents</span>
          </div>
          <div className="flex items-center gap-4">
            <IndexStatus />
            <a href="https://github.com/Tesserax-labs/tesserax-miner" className="inline-flex items-center gap-1 hover:text-fg" target="_blank" rel="noreferrer">
              <Pickaxe size={14} /> Miner
            </a>
          </div>
        </div>
      </footer>
      <ScrollRestoration />
    </div>
  );
}

/** Page width and padding. */
export function Page({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cx("mx-auto max-w-7xl px-4 py-6 sm:px-6 sm:py-8", className)}>{children}</div>;
}
