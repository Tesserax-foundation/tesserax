// Dashboard of the pool. Everything shown comes from its read-only API; text from miners, such
// as worker names, is inserted as text only.

// The theme applies before the first paint.
(() => {
  let theme = null;
  try {
    theme = localStorage.getItem("theme");
  } catch {}
  const dark = theme ? theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
  document.documentElement.classList.toggle("dark", dark);
})();

const PLANCK = 10n ** 12n;
let info = null;
let explorer = null;

const $ = (id) => document.getElementById(id);

/** Release of the Tesserax build of XMRig the dashboard links to. */
const MINER = "6.26.0-tesserax.1";
const RELEASE = `https://github.com/Tesserax-foundation/tesserax-miner/releases/download/v${MINER}`;

/** The system of this computer, for the download offered first. */
function detectSystem() {
  const agent = navigator.userAgent;
  if (/Windows/i.test(agent)) return "windows";
  if (/Mac/i.test(agent)) return "macos-arm64";
  if (/aarch64|arm64/i.test(agent)) return "linux-arm64";
  return "linux-x64";
}

let system = detectSystem();

/** The download, the command and the notes of the getting started steps for `system`. */
function showStart() {
  const windows = system === "windows";
  const archive = `tesserax-miner-${MINER}-${system}.${windows ? "zip" : "tar.gz"}`;
  for (const button of document.querySelectorAll("[data-os]")) button.setAttribute("aria-selected", String(button.dataset.os === system));
  $("download").href = `${RELEASE}/${archive}`;
  $("download").textContent = "Download the miner";
  $("unpack").textContent = windows ? "Unzip it, then open a terminal in its folder." : `Unpack it: tar xzf ${archive}, then cd into its folder.`;
  const stratum = info?.stratum ?? "…";
  const address = $("address").value.trim() || "YOUR_ADDRESS";
  $("start-command").textContent = `${windows ? "xmrig.exe" : "./xmrig"} -a rx/tess -o ${stratum} -u ${address}.rig1 -p x`;
  $("start-note").textContent = system.startsWith("macos")
    ? "Allow it once before: xattr -d com.apple.quarantine xmrig. Mining at full speed takes about 2.5 GB of memory."
    : windows
      ? "Windows Defender may flag any XMRig build as a miner: allow it. Mining at full speed takes about 2.5 GB of memory."
      : "Mining at full speed takes about 2.5 GB of memory; huge pages make it faster (sudo sysctl vm.nr_hugepages=1280).";
}

function tess(planck, decimals = 4) {
  const value = BigInt(planck ?? 0);
  const whole = value / PLANCK;
  const fraction = (value % PLANCK).toString().padStart(12, "0").slice(0, decimals).replace(/0+$/, "");
  return `${whole.toLocaleString("en-US")}${fraction ? "." + fraction : ""} TESS`;
}

function hashrate(value) {
  const units = ["H/s", "kH/s", "MH/s", "GH/s", "TH/s"];
  let n = Number(value) || 0;
  let unit = 0;
  while (n >= 1000 && unit < units.length - 1) {
    n /= 1000;
    unit++;
  }
  return `${n.toLocaleString("en-US", { maximumFractionDigits: n < 10 ? 2 : 1 })} ${units[unit]}`;
}

function ago(ms) {
  if (!ms) return "—";
  const seconds = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (seconds < 60) return `${seconds} s ago`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

const short = (text, head = 8, tail = 6) => (text && text.length > head + tail + 1 ? `${text.slice(0, head)}…${text.slice(-tail)}` : text ?? "—");

/** A cell: text, or a link to the explorer when it has a path. */
function cell(text, path, mono) {
  const td = document.createElement("td");
  if (path && explorer) {
    const a = document.createElement("a");
    a.href = explorer + path;
    a.target = "_blank";
    a.rel = "noreferrer noopener";
    a.textContent = text;
    td.append(a);
  } else {
    td.textContent = text;
  }
  if (mono) td.classList.add("mono");
  return td;
}

function badge(status) {
  const td = document.createElement("td");
  const span = document.createElement("span");
  span.className = `badge ${status}`;
  span.textContent = status;
  td.append(span);
  return td;
}

function fill(tbody, rows, columns, empty) {
  tbody.replaceChildren();
  if (!rows.length) {
    const tr = document.createElement("tr");
    const td = document.createElement("td");
    td.colSpan = columns;
    td.className = "empty";
    td.textContent = empty;
    tr.append(td);
    tbody.append(tr);
    return;
  }
  for (const cells of rows) {
    const tr = document.createElement("tr");
    tr.append(...cells);
    tbody.append(tr);
  }
}

async function json(path) {
  const response = await fetch(path, { cache: "no-store" });
  if (!response.ok) throw new Error((await response.json().catch(() => ({}))).error ?? response.statusText);
  return response.json();
}

async function refresh() {
  try {
    const [stats, blocks, payments] = await Promise.all([json("/api/stats"), json("/api/blocks?limit=25"), json("/api/payments?limit=25")]);
    info = stats.pool;
    explorer = info.explorer;
    document.title = info.name;
    $("pool-name").textContent = info.name;
    $("stratum").textContent = info.stratum;
    $("command").textContent = `xmrig -a rx/tess -o ${info.stratum} -u ${$("address").value.trim() || "YOUR_ADDRESS"}.rig1 -p x`;
    $("maturity").textContent = String(info.maturity);
    $("test-note").hidden = !/test/i.test(info.network ?? "");
    $("test-network").textContent = (info.network ?? "testnet").toLowerCase();
    showStart();
    $("hashrate").textContent = hashrate(stats.hashrate);
    $("miners").textContent = stats.miners.toLocaleString("en-US");
    $("workers").textContent = `${stats.workers} rig${stats.workers === 1 ? "" : "s"} connected`;
    $("blocks").textContent = stats.blocks.toLocaleString("en-US");
    $("last-block").textContent = stats.last_block_at ? `Last ${ago(stats.last_block_at)}` : "None yet";
    $("effort").textContent = stats.round_effort == null ? "—" : `${stats.round_effort.toFixed(1)} %`;
    $("fee").textContent = `${info.fee_percent} %`;
    $("terms").textContent = `PPLNS ×${info.pplns_factor} · payouts from ${tess(info.min_payout, 2)}`;
    $("footer-terms").textContent = `Rewards credited ${info.maturity} blocks deep, paid every ${info.payout_interval_minutes} min to balances of ${tess(info.min_payout, 2)} or more.`;
    const network = $("network");
    if (stats.network) {
      network.className = "pill online";
      $("network-text").textContent = `${info.network ? `${info.network} · ` : ""}Block #${stats.network.height.toLocaleString("en-US")}`;
      $("network-hashrate").textContent = hashrate(stats.network.hashrate);
      $("difficulty").textContent = `Difficulty ${Number(stats.network.difficulty).toLocaleString("en-US")}`;
      const share = stats.network.hashrate > 0 ? (stats.hashrate / stats.network.hashrate) * 100 : 0;
      $("share").textContent = `${share.toFixed(1)} % of the network`;
    } else {
      network.className = "pill offline";
      $("network-text").textContent = "Node offline";
    }
    fill(
      $("blocks-table"),
      blocks.map((b) => [
        cell(`#${b.height.toLocaleString("en-US")}`, b.hash ? `/block/${b.hash}` : null),
        badge(b.status),
        cell(b.reward ? tess(b.reward) : "—"),
        cell(`${b.effort.toFixed(0)} %`),
        cell(short(b.finder), `/account/${b.finder}`, true),
        cell(ago(b.found_at)),
      ]),
      6,
      "No block found yet",
    );
    fill(
      $("payments-table"),
      payments.map((p) => [
        cell(short(p.address), `/account/${p.address}`, true),
        cell(tess(p.amount)),
        badge(p.status),
        cell(p.tx_hash ? short(p.tx_hash, 10, 6) : "—", p.tx_hash ? `/extrinsic/${p.tx_hash}` : null, true),
        cell(ago(p.created_at)),
      ]),
      5,
      "No payout yet",
    );
  } catch (error) {
    $("network").className = "pill offline";
    $("network-text").textContent = "Pool unreachable";
  }
  const address = $("address").value.trim();
  if (address) await showMiner(address);
}

async function showMiner(address) {
  const error = $("miner-error");
  try {
    const miner = await json(`/api/miners/${encodeURIComponent(address)}`);
    error.hidden = true;
    $("miner").hidden = false;
    $("miner-hashrate").textContent = hashrate(miner.hashrate);
    $("miner-balance").textContent = tess(miner.balance);
    $("miner-immature").textContent = tess(miner.immature);
    $("miner-paid").textContent = tess(miner.paid);
    fill(
      $("miner-workers"),
      miner.workers.map((w) => [
        cell(w.name),
        cell(hashrate(w.hashrate)),
        cell(w.difficulty ? w.difficulty.toLocaleString("en-US") : "—"),
        cell(String(w.shares)),
        (() => {
          const td = cell(w.online ? ago(w.last_share_at) : "Offline");
          if (!w.online) td.classList.add("muted");
          return td;
        })(),
      ]),
      5,
      "No rig of this address in the last 10 minutes",
    );
    fill(
      $("miner-payments"),
      miner.payments.map((p) => [cell(tess(p.amount)), badge(p.status), cell(p.tx_hash ? short(p.tx_hash, 10, 6) : "—", p.tx_hash ? `/extrinsic/${p.tx_hash}` : null, true), cell(ago(p.created_at))]),
      4,
      "No payout yet",
    );
  } catch (e) {
    $("miner").hidden = true;
    error.hidden = false;
    error.textContent = e.message === "not a Tesserax address" ? "Not a Tesserax address: they start with “te”." : "The pool does not answer.";
  }
}

document.addEventListener("DOMContentLoaded", () => {
  $("theme").addEventListener("click", () => {
    const dark = !document.documentElement.classList.contains("dark");
    document.documentElement.classList.toggle("dark", dark);
    try {
      localStorage.setItem("theme", dark ? "dark" : "light");
    } catch {}
  });
  for (const button of document.querySelectorAll("[data-copy]")) {
    button.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText($(button.dataset.copy).textContent);
        button.textContent = "Copied";
        setTimeout(() => (button.textContent = "Copy"), 1500);
      } catch {}
    });
  }
  for (const button of document.querySelectorAll("[data-os]")) {
    button.addEventListener("click", () => {
      system = button.dataset.os;
      showStart();
    });
  }
  let saved = decodeURIComponent(location.hash.slice(1));
  if (!saved) {
    try {
      saved = localStorage.getItem("address") ?? "";
    } catch {}
  }
  $("address").value = saved;
  showStart();
  $("lookup").addEventListener("submit", (event) => {
    event.preventDefault();
    const address = $("address").value.trim();
    try {
      localStorage.setItem("address", address);
    } catch {}
    history.replaceState(null, "", address ? `#${address}` : location.pathname);
    void refresh();
  });
  void refresh();
  setInterval(refresh, 15_000);
});
