import "./style.css";
import "./mine.css";
import { startField } from "./field";
import { startLive } from "./live";
import { startPage } from "./page";

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

startField(document.getElementById("field") as HTMLCanvasElement, reducedMotion);
startLive();
startPage(reducedMotion);

// The miner release and the pool the guide points to.

const RELEASE = "6.26.0-tesserax.1";
const DOWNLOADS = `https://github.com/Tesserax-foundation/tesserax-miner/releases/download/v${RELEASE}`;
const STRATUM = "testnet-stratum.tesserax.app:25202";
const POOL = "https://testnet-pool.tesserax.app";

type System = "windows-x64" | "macos-arm64" | "macos-x64" | "linux-x64" | "linux-arm64";

const NAMES: Record<System, string> = {
  "windows-x64": "Windows",
  "macos-arm64": "macOS · Apple Silicon",
  "macos-x64": "macOS · Intel",
  "linux-x64": "Linux · x64",
  "linux-arm64": "Linux · ARM64",
};

// Base58 without 0, O, I and l; Tesserax addresses start with "te".
const ADDRESS = /^te[1-9A-HJ-NP-Za-km-z]{40,56}$/;
const WORKER = /^[A-Za-z0-9_-]{1,32}$/;

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const addressInput = $<HTMLInputElement>("address");
const workerInput = $<HTMLInputElement>("worker");
const lightInput = $<HTMLInputElement>("light");
const addressHint = $("address-hint");

function detect(): System {
  const platform = (
    (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform ??
    navigator.platform ??
    ""
  ).toLowerCase();
  const agent = navigator.userAgent.toLowerCase();
  if (platform.includes("win") || agent.includes("windows")) return "windows-x64";
  // Browsers report Apple Silicon Macs as Intel; nearly every Mac sold since 2021 is Apple Silicon.
  if (platform.includes("mac") || agent.includes("mac os")) return "macos-arm64";
  if (agent.includes("aarch64") || agent.includes("arm64")) return "linux-arm64";
  return "linux-x64";
}

function read(key: string): string {
  try {
    return localStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Remembering is a convenience only.
  }
}

function escape(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** A command line as HTML: plain text, with the parts the user provides highlighted. */
function line(...parts: (string | { value: string; filled: boolean })[]): string {
  return parts
    .map((part) =>
      typeof part === "string"
        ? escape(part)
        : `<span class="${part.filled ? "var" : "var empty"}">${escape(part.value)}</span>`,
    )
    .join("");
}

let system: System = (read("mine-os") as System) in NAMES ? (read("mine-os") as System) : detect();

function render(): void {
  const windows = system === "windows-x64";
  const mac = system.startsWith("macos");
  const folder = `tesserax-miner-${RELEASE}-${system}`;
  const file = `${folder}.${windows ? "zip" : "tar.gz"}`;
  const url = `${DOWNLOADS}/${file}`;

  for (const button of document.querySelectorAll<HTMLButtonElement>("[data-os]")) {
    const selected = button.dataset.os === system;
    button.classList.toggle("on", selected);
    button.setAttribute("aria-checked", String(selected));
  }

  // Step 2: the download.
  const download = $<HTMLAnchorElement>("download");
  download.href = url;
  $("download-label").textContent = `Download for ${NAMES[system]}`;
  $("download-file").textContent = file;
  $("shell-name").textContent = windows ? "PowerShell" : "a terminal";
  $("term-title-2").textContent = windows ? "PowerShell" : "terminal";
  $("term-title-3").textContent = windows ? "PowerShell" : "terminal";

  const download_lines = windows
    ? [
        line("cd ~\\Downloads"),
        line(`curl.exe -LO ${url}`),
        line(`Expand-Archive ${file} -DestinationPath .`),
        line(`cd ${folder}`),
      ]
    : [
        line(`curl -LO ${url}`),
        line(`tar -xzf ${file}`),
        line(`cd ${folder}`),
        // Lets macOS run a binary downloaded from the internet.
        ...(mac ? [line("xattr -c xmrig")] : []),
      ];
  $("cmd-download").innerHTML = download_lines.join("\n");

  // Step 3: the command, with the address and the machine name.
  const address = addressInput.value.trim();
  const worker = workerInput.value.trim();
  const addressOk = ADDRESS.test(address);
  const workerOk = WORKER.test(worker);
  const login = [
    { value: addressOk ? address : "<your address>", filled: addressOk },
    ".",
    { value: workerOk ? worker : "rig1", filled: workerOk },
  ];
  const parts: (string | { value: string; filled: boolean })[] = [
    `${windows ? ".\\xmrig.exe" : "./xmrig"} -a rx/tess -o ${STRATUM} -u `,
    ...login,
    " -p x",
  ];
  if (lightInput.checked) parts.push(" --randomx-mode=light");
  $("cmd-start").innerHTML = line(...parts);

  addressHint.classList.toggle("bad", address !== "" && !addressOk);
  addressHint.classList.toggle("good", addressOk);
  addressHint.textContent =
    address === ""
      ? "Paste it here: the commands below fill themselves in."
      : addressOk
        ? "Address set: the command below is ready to copy."
        : "This is not a Tesserax address: copy it from the wallet, it starts with te.";

  $<HTMLAnchorElement>("dashboard").href = addressOk ? `${POOL}/#${address}` : POOL;
}

for (const button of document.querySelectorAll<HTMLButtonElement>("[data-os]")) {
  button.addEventListener("click", () => {
    system = button.dataset.os as System;
    write("mine-os", system);
    render();
  });
}

addressInput.value = read("mine-address");
workerInput.value = read("mine-worker") || "rig1";
addressInput.addEventListener("input", () => {
  write("mine-address", addressInput.value.trim());
  render();
});
workerInput.addEventListener("input", () => {
  write("mine-worker", workerInput.value.trim());
  render();
});
lightInput.addEventListener("change", render);

for (const button of document.querySelectorAll<HTMLButtonElement>("[data-copy]")) {
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($(button.dataset.copy!).textContent ?? "");
      button.textContent = "Copied";
    } catch {
      button.textContent = "Select and copy";
    }
    setTimeout(() => (button.textContent = "Copy"), 1600);
  });
}

render();

// Live figures of the pool.

function hashrate(value: number): string {
  const units = ["H/s", "kH/s", "MH/s", "GH/s"];
  let i = 0;
  while (value >= 1000 && i < units.length - 1) {
    value /= 1000;
    i++;
  }
  return `${value.toLocaleString("en-US", { maximumFractionDigits: value < 10 ? 2 : value < 100 ? 1 : 0 })} ${units[i]}`;
}

async function followPool(): Promise<void> {
  try {
    const response = await fetch(`${POOL}/api/stats`, { cache: "no-store" });
    const stats = (await response.json()) as { miners?: number; network?: { hashrate?: number } };
    const set = (name: string, text: string) => {
      const element = document.querySelector<HTMLElement>(`[data-pool="${name}"]`);
      if (element) element.textContent = text;
    };
    if (typeof stats.network?.hashrate === "number") set("network", hashrate(stats.network.hashrate));
    if (typeof stats.miners === "number") set("miners", stats.miners.toLocaleString("en-US"));
  } catch {
    // Kept as it was: the next attempt may succeed.
  }
  setTimeout(followPool, 30000);
}

void followPool();
