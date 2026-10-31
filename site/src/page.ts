// What every page of the site shares: the navigation, the light following the pointer, and the
// elements revealed as they come into view.

const GLYPHS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#%&*+<>/\\=";

export function startPage(reducedMotion: boolean): void {
  startNav();
  startGlow();
  startReveal(reducedMotion);
}

// Navigation: a solid bar once scrolled, a menu on small screens.

function startNav(): void {
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
}

// The light following the pointer, on the page and inside the cards.

function startGlow(): void {
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
}

// Headings decoded from noise.

function scramble(element: HTMLElement, reducedMotion: boolean): void {
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

function count(element: HTMLElement, reducedMotion: boolean): void {
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

function startReveal(reducedMotion: boolean): void {
  for (const group of document.querySelectorAll(".cards, .legend, .timeline, .live-grid, .mine-steps")) {
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
        if (element.matches("[data-scramble]")) scramble(element, reducedMotion);
        for (const heading of element.querySelectorAll<HTMLElement>("[data-scramble]")) scramble(heading, reducedMotion);
        for (const number of element.querySelectorAll<HTMLElement>("[data-count]")) count(number, reducedMotion);
      }
    },
    { threshold: 0.18, rootMargin: "0px 0px -40px 0px" },
  );
  for (const element of document.querySelectorAll(".reveal")) observer.observe(element);
  for (const heading of document.querySelectorAll<HTMLElement>("h1 [data-scramble]")) scramble(heading, reducedMotion);
}
