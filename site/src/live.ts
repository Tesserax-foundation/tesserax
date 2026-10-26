// Live figures of the testnet: the best block from the public RPC, the supply from the explorer.

const RPC = "wss://rpc.tesserax.app:21207";
const SUPPLY = "https://explorer.tesserax.app/api/supply";

function set(name: string, text: string, flash = false): void {
  for (const element of document.querySelectorAll<HTMLElement>(`[data-live="${name}"]`)) {
    if (element.textContent === text) continue;
    element.textContent = text;
    if (flash) {
      element.classList.remove("flash");
      void element.offsetWidth;
      element.classList.add("flash");
    }
  }
}

let height: number | undefined;

/** The best block seen, for the miner console. */
export function bestBlock(): number | undefined {
  return height;
}

function followBlocks(): void {
  let lastBlock = 0;
  let retry = 2000;
  const dot = document.querySelector("#status .dot");

  setInterval(() => {
    if (!lastBlock) return;
    const seconds = Math.round((Date.now() - lastBlock) / 1000);
    set("age", seconds < 2 ? "mined just now" : `last block ${seconds} s ago`);
  }, 1000);

  function connect(): void {
    let socket: WebSocket;
    try {
      socket = new WebSocket(RPC);
    } catch {
      return;
    }
    socket.onopen = () => {
      retry = 2000;
      dot?.classList.remove("off");
      socket.send(JSON.stringify({ id: 1, jsonrpc: "2.0", method: "chain_subscribeNewHeads", params: [] }));
    };
    socket.onmessage = (message) => {
      const data = JSON.parse(message.data as string);
      const number = data?.params?.result?.number;
      if (typeof number !== "string") return;
      const value = parseInt(number, 16);
      if (height !== undefined && value <= height) return;
      height = value;
      lastBlock = Date.now();
      set("height", `#${value.toLocaleString("en-US")}`, true);
    };
    socket.onclose = () => {
      dot?.classList.add("off");
      setTimeout(connect, retry);
      retry = Math.min(retry * 2, 60000);
    };
  }

  connect();
}

async function followSupply(): Promise<void> {
  try {
    const response = await fetch(SUPPLY);
    const supply = (await response.json()) as { total?: string };
    const total = Number(supply.total);
    if (Number.isFinite(total)) set("total", Math.floor(total).toLocaleString("en-US"));
  } catch {
    // Kept as it was: the next attempt may succeed.
  }
  setTimeout(followSupply, 60000);
}

export function startLive(): void {
  followBlocks();
  void followSupply();
}
