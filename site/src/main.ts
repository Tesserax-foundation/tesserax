import "./style.css";
import { startCode } from "./code";
import { startField } from "./field";
import { bestBlock, startLive } from "./live";
import { startTesseract } from "./tesseract";

const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

startField(document.getElementById("field") as HTMLCanvasElement, reducedMotion);
startTesseract(document.getElementById("tesseract") as HTMLCanvasElement, reducedMotion);
startLive();
startCode(reducedMotion);

// Navigation: a solid bar once scrolled, a menu on small screens.

const nav = document.getElementById("nav")!;
const menu = document.getElementById("menu")!;
const links = document.querySelector(".nav-links")!;
const onScroll = () => nav.classList.toggle("scrolled", window.scrollY > 20);
window.addEventListener("scroll", onScroll, { passive: true });
onScroll();
menu.addEventListener("click", () => {
  const open = links.classList.toggle("open");
  menu.setAttribute("aria-expanded", String(open));
});
for (const link of links.querySelectorAll("a")) {
  link.addEventListener("click", () => {
    links.classList.remove("open");
    menu.setAttribute("aria-expanded", "false");
  });
}

// The light following the pointer, on the page and inside the cards.

const glow = document.querySelector<HTMLElement>(".glow")!;
window.addEventListener("pointermove", (e) => {
  glow.style.setProperty("--mx", `${e.clientX}px`);
  glow.style.setProperty("--my", `${e.clientY}px`);
});
for (const card of document.querySelectorAll<HTMLElement>(".card")) {
  card.addEventListener("pointermove", (e) => {
    const rect = card.getBoundingClientRect();
    card.style.setProperty("--cx", `${e.clientX - rect.left}px`);
    card.style.setProperty("--cy", `${e.clientY - rect.top}px`);
  });
}

// Headings decoded from noise.

const GLYPHS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#%&*+<>/\\=";

function scramble(element: HTMLElement): void {
  const target = element.textContent ?? "";
  if (reducedMotion) return;
  const duration = 900;
  const begin = performance.now();
  const step = (now: number) => {
    const progress = Math.min(1, (now - begin) / duration);
    const settled = Math.floor(progress * target.length);
    let text = target.slice(0, settled);
    for (let i = settled; i < target.length; i++) {
      text += target[i] === " " ? " " : GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
    }
    element.textContent = text;
    if (progress < 1) requestAnimationFrame(step);
    else element.textContent = target;
  };
  requestAnimationFrame(step);
}

// Numbers counted up.

function count(element: HTMLElement): void {
  const target = Number(element.dataset.count);
  const decimals = Number(element.dataset.decimals ?? 0);
  const format = (value: number) =>
    value.toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
  if (reducedMotion) {
    element.textContent = format(target);
    return;
  }
  const duration = 1600;
  const begin = performance.now();
  const step = (now: number) => {
    const progress = Math.min(1, (now - begin) / duration);
    const eased = 1 - Math.pow(1 - progress, 4);
    element.textContent = format(target * eased);
    if (progress < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}

// Elements revealed as they come into view, siblings one after the other.

for (const group of document.querySelectorAll(".cards, .legend, .timeline, .live-grid")) {
  [...group.querySelectorAll<HTMLElement>(":scope > .reveal")].forEach((element, i) =>
    element.style.setProperty("--delay", `${i * 0.08}s`),
  );
}

const observer = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const element = entry.target as HTMLElement;
      observer.unobserve(element);
      element.classList.add("visible");
      if (element.matches("[data-scramble]")) scramble(element);
      for (const heading of element.querySelectorAll<HTMLElement>("[data-scramble]")) scramble(heading);
      for (const number of element.querySelectorAll<HTMLElement>("[data-count]")) count(number);
    }
  },
  { threshold: 0.18, rootMargin: "0px 0px -40px 0px" },
);
for (const element of document.querySelectorAll(".reveal")) observer.observe(element);
for (const heading of document.querySelectorAll<HTMLElement>("h1 [data-scramble]")) scramble(heading);

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
