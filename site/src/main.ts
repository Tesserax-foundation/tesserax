import "./style.css";
import { startCode } from "./code";
import { startField } from "./field";
import { bestBlock, startLive } from "./live";
import { startPage } from "./page";
import { startTesseract } from "./tesseract";

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

startField(document.getElementById("field") as HTMLCanvasElement, reducedMotion);
startTesseract(document.getElementById("tesseract") as HTMLCanvasElement, reducedMotion);
startLive();
startCode(reducedMotion);
startPage(reducedMotion);

// The miner console of the mining section.

const consoleBody = document.getElementById("console")!;
const lines: string[] = [];
let shares = 0;
let consoleVisible = false;
new IntersectionObserver(([entry]) => (consoleVisible = entry.isIntersecting)).observe(consoleBody);

function hex(length: number): string {
  let text = "";
  for (let i = 0; i < length; i++) text += Math.floor(Math.random() * 16).toString(16);
  return text;
}

function time(): string {
  return new Date().toTimeString().slice(0, 8);
}

function consoleLine(): string {
  const roll = Math.random();
  const height = bestBlock() ?? 12_000;
  if (roll < 0.18) {
    return `<span class="t">[${time()}]</span> <span class="hi">net</span>  new job from testnet-pool.tesserax.app diff 120001 algo rx/tess height ${height + 1}`;
  }
  if (roll < 0.55) {
    shares++;
    return `<span class="t">[${time()}]</span> <span class="hi">cpu</span>  <span class="ok">accepted</span> (${shares}/0) diff 120001 (${20 + Math.floor(Math.random() * 60)} ms)`;
  }
  if (roll < 0.8) {
    return `<span class="t">[${time()}]</span> <span class="hi">cpu</span>  nonce 0x${hex(8)} hash 0000${hex(28)}…`;
  }
  const speed = (380 + Math.random() * 60).toFixed(1);
  return `<span class="t">[${time()}]</span> <span class="hi">miner</span> speed 10s/60s/15m ${speed} ${(390).toFixed(1)} n/a H/s`;
}

function tick(): void {
  if (consoleVisible || lines.length === 0) {
    lines.push(consoleLine());
    if (lines.length > 22) lines.shift();
    consoleBody.innerHTML = lines.join("\n");
  }
  setTimeout(tick, reducedMotion ? 4000 : 650 + Math.random() * 700);
}

for (let i = 0; i < 14; i++) lines.push(consoleLine());
consoleBody.innerHTML = lines.join("\n");
tick();
