// Page of the faucet. Everything shown comes from its API.

// The theme applies before the first paint.
(() => {
  let theme = null;
  try {
    theme = localStorage.getItem("theme");
  } catch {}
  const dark = theme ? theme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
  document.documentElement.classList.toggle("dark", dark);
})();

const $ = (id) => document.getElementById(id);
let info = null;

const tess = (text) => `${Number(text).toLocaleString("en-US", { maximumFractionDigits: 4 })} TESS`;

function link(path, text) {
  const a = document.createElement("a");
  a.href = `${info.explorer}${path}`;
  a.target = "_blank";
  a.rel = "noreferrer noopener";
  a.textContent = text;
  return a;
}

async function refresh() {
  try {
    const response = await fetch("/api/info");
    info = await response.json();
    $("network").classList.toggle("online", info.balance !== null);
    $("network-text").textContent = info.balance !== null ? "Testnet" : "Node unreachable";
    $("amount").textContent = tess(info.amount);
    $("per-claim").textContent = tess(info.amount);
    $("given").textContent = tess(info.given_today);
    $("cap").textContent = `Up to ${tess(info.daily_cap)}`;
    $("balance").textContent = info.balance !== null ? tess(info.balance) : "—";
    $("faucet-address").textContent = info.address;
  } catch {
    $("network-text").textContent = "Offline";
  }
}

async function claim(event) {
  event.preventDefault();
  const address = $("address").value.trim();
  const button = $("send");
  $("result").hidden = true;
  $("error").hidden = true;
  if (!address.startsWith("te")) {
    $("error").textContent = "Not a Tesserax address: they start with “te”.";
    $("error").hidden = false;
    return;
  }
  button.disabled = true;
  const label = button.innerHTML;
  button.textContent = "Sending… about 20 s";
  try {
    const response = await fetch("/api/claim", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ address }),
    });
    const body = await response.json();
    if (!response.ok) throw new Error(body.error ?? "the faucet does not answer");
    const result = $("result");
    result.replaceChildren(`${tess(body.amount)} sent in block #${body.block_number}. `);
    if (info?.explorer) result.append(link(`/account/${body.address}`, "See the account"));
    result.hidden = false;
    void refresh();
  } catch (e) {
    $("error").textContent = e.message.charAt(0).toUpperCase() + e.message.slice(1) + ".";
    $("error").hidden = false;
  } finally {
    button.disabled = false;
    button.innerHTML = label;
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
  $("address").value = decodeURIComponent(location.hash.slice(1));
  $("claim").addEventListener("submit", claim);
  void refresh();
  setInterval(refresh, 30_000);
});
