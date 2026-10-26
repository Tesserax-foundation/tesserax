// The hero: a tesseract, the four-dimensional cube the chain is named after, rotating in 4D and
// projected to the screen, with packets of light running along its edges.

type Vec4 = [number, number, number, number];

const VERTICES: Vec4[] = Array.from({ length: 16 }, (_, i) => [
  i & 1 ? 1 : -1,
  i & 2 ? 1 : -1,
  i & 4 ? 1 : -1,
  i & 8 ? 1 : -1,
]);

const EDGES: [number, number][] = [];
for (let a = 0; a < 16; a++) {
  for (let b = a + 1; b < 16; b++) {
    const diff = a ^ b;
    if ((diff & (diff - 1)) === 0) EDGES.push([a, b]);
  }
}

const BLUE = [47, 107, 255];
const CYAN = [56, 225, 255];
const VIOLET = [139, 92, 255];

function mix(a: number[], b: number[], t: number): string {
  return `${Math.round(a[0] + (b[0] - a[0]) * t)}, ${Math.round(a[1] + (b[1] - a[1]) * t)}, ${Math.round(a[2] + (b[2] - a[2]) * t)}`;
}

function rotate(v: Vec4, i: number, j: number, angle: number): void {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  const a = v[i];
  const b = v[j];
  v[i] = a * c - b * s;
  v[j] = a * s + b * c;
}

interface Packet {
  edge: number;
  t: number;
  speed: number;
  forward: boolean;
}

export function startTesseract(canvas: HTMLCanvasElement, reducedMotion: boolean): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  let width = 0;
  let height = 0;
  let visible = true;
  let pointerX = 0;
  let pointerY = 0;
  let tiltX = 0;
  let tiltY = 0;
  const start = performance.now();

  const packets: Packet[] = Array.from({ length: 14 }, () => ({
    edge: Math.floor(Math.random() * EDGES.length),
    t: Math.random(),
    speed: 0.35 + Math.random() * 0.5,
    forward: Math.random() > 0.5,
  }));

  function resize(): void {
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const rect = canvas.getBoundingClientRect();
    width = rect.width;
    height = rect.height;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx!.setTransform(ratio, 0, 0, ratio, 0, 0);
    if (reducedMotion) requestAnimationFrame(frame);
  }

  window.addEventListener("resize", resize);
  window.addEventListener("pointermove", (e) => {
    pointerX = e.clientX / window.innerWidth - 0.5;
    pointerY = e.clientY / window.innerHeight - 0.5;
  });
  new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    if (visible) requestAnimationFrame(frame);
  }).observe(canvas);
  resize();

  let last = start;

  function frame(now: number): void {
    if (!visible) return;
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;
    const time = reducedMotion ? 6 : (now - start) / 1000;

    tiltX += (pointerY * 0.6 - tiltX) * 0.04;
    tiltY += (pointerX * 0.8 - tiltY) * 0.04;

    // Rotations in the planes that involve w turn the cube inside out; the others tumble it.
    const projected = VERTICES.map((vertex) => {
      const v: Vec4 = [...vertex];
      rotate(v, 0, 3, time * 0.42);
      rotate(v, 1, 3, time * 0.27);
      rotate(v, 2, 3, time * 0.19);
      rotate(v, 0, 1, time * 0.11);
      const w = 1 / (3 - v[3]);
      const p = [v[0] * w, v[1] * w, v[2] * w];
      const q: Vec4 = [p[0], p[1], p[2], 0];
      rotate(q, 1, 2, 0.5 + tiltX);
      rotate(q, 0, 2, 0.35 + tiltY);
      const z = 1 / (2.6 - q[2]);
      const size = Math.min(width, height) * 1.05;
      return {
        x: width / 2 + q[0] * z * size,
        y: height / 2 + q[1] * z * size,
        depth: (v[3] + 1) / 2,
        scale: z,
      };
    });

    ctx!.clearRect(0, 0, width, height);
    ctx!.globalCompositeOperation = "lighter";
    ctx!.lineCap = "round";

    for (const [a, b] of EDGES) {
      const p = projected[a];
      const q = projected[b];
      const depth = (p.depth + q.depth) / 2;
      const color = depth > 0.5 ? mix(CYAN, VIOLET, (depth - 0.5) * 0.8) : mix(BLUE, CYAN, depth * 2);
      const alpha = 0.25 + depth * 0.55;

      ctx!.strokeStyle = `rgba(${color}, ${alpha * 0.12})`;
      ctx!.lineWidth = 9 * ((p.scale + q.scale) / 2);
      ctx!.beginPath();
      ctx!.moveTo(p.x, p.y);
      ctx!.lineTo(q.x, q.y);
      ctx!.stroke();

      ctx!.strokeStyle = `rgba(${color}, ${alpha})`;
      ctx!.lineWidth = 1.3 + depth * 0.9;
      ctx!.beginPath();
      ctx!.moveTo(p.x, p.y);
      ctx!.lineTo(q.x, q.y);
      ctx!.stroke();
    }

    for (const p of projected) {
      const radius = 2 + p.depth * 3;
      const glow = ctx!.createRadialGradient(p.x, p.y, 0, p.x, p.y, radius * 6);
      glow.addColorStop(0, `rgba(${mix(BLUE, CYAN, p.depth)}, 0.55)`);
      glow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx!.fillStyle = glow;
      ctx!.beginPath();
      ctx!.arc(p.x, p.y, radius * 6, 0, Math.PI * 2);
      ctx!.fill();
      ctx!.fillStyle = "rgba(235, 248, 255, 0.95)";
      ctx!.beginPath();
      ctx!.arc(p.x, p.y, radius * 0.55, 0, Math.PI * 2);
      ctx!.fill();
    }

    for (const packet of packets) {
      if (!reducedMotion) packet.t += packet.speed * dt;
      if (packet.t >= 1) {
        // Continue from the vertex reached along another of its edges.
        const [a, b] = EDGES[packet.edge];
        const at = packet.forward ? b : a;
        const next = EDGES.map((edge, i) => [edge, i] as const).filter(([[x, y], i]) => i !== packet.edge && (x === at || y === at));
        const [[x], i] = next[Math.floor(Math.random() * next.length)];
        packet.edge = i;
        packet.forward = x === at;
        packet.t = 0;
      }
      const [a, b] = EDGES[packet.edge];
      const from = projected[packet.forward ? a : b];
      const to = projected[packet.forward ? b : a];
      const x = from.x + (to.x - from.x) * packet.t;
      const y = from.y + (to.y - from.y) * packet.t;
      const glow = ctx!.createRadialGradient(x, y, 0, x, y, 14);
      glow.addColorStop(0, "rgba(160, 240, 255, 0.9)");
      glow.addColorStop(0.3, "rgba(56, 225, 255, 0.35)");
      glow.addColorStop(1, "rgba(0, 0, 0, 0)");
      ctx!.fillStyle = glow;
      ctx!.beginPath();
      ctx!.arc(x, y, 14, 0, Math.PI * 2);
      ctx!.fill();
    }

    ctx!.globalCompositeOperation = "source-over";
    if (!reducedMotion) requestAnimationFrame(frame);
  }

  requestAnimationFrame(frame);
}
