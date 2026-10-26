// The background: a lattice of points — the structure post-quantum cryptography is built on —
// crossed by slow waves, lighting up and linking around the pointer.

export function startField(canvas: HTMLCanvasElement, reducedMotion: boolean): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const spacing = 44;
  let width = 0;
  let height = 0;
  let columns = 0;
  let rows = 0;
  let pointerX = -1000;
  let pointerY = -1000;
  let scroll = 0;
  let last = 0;

  function resize(): void {
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    width = window.innerWidth;
    height = window.innerHeight;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
    ctx!.setTransform(ratio, 0, 0, ratio, 0, 0);
    columns = Math.ceil(width / spacing) + 2;
    rows = Math.ceil(height / spacing) + 2;
  }

  window.addEventListener("resize", resize);
  window.addEventListener("pointermove", (e) => {
    pointerX = e.clientX;
    pointerY = e.clientY;
  });
  window.addEventListener("pointerleave", () => {
    pointerX = -1000;
    pointerY = -1000;
  });
  window.addEventListener("scroll", () => (scroll = window.scrollY), { passive: true });
  resize();

  function frame(now: number): void {
    // About 30 frames a second is plenty for a background.
    if (now - last < 32 && !reducedMotion) {
      requestAnimationFrame(frame);
      return;
    }
    last = now;
    const time = reducedMotion ? 0 : now / 1000;
    ctx!.clearRect(0, 0, width, height);

    const offset = (scroll * 0.25) % spacing;
    const near: [number, number, number][] = [];

    for (let row = -1; row < rows; row++) {
      for (let column = -1; column < columns; column++) {
        const x = column * spacing + (row % 2 ? spacing / 2 : 0);
        const y = row * spacing - offset;
        const wave = Math.sin(x * 0.006 + y * 0.004 - time * 0.6) * Math.sin(y * 0.005 - time * 0.35);
        const dx = x - pointerX;
        const dy = y - pointerY;
        const distance = Math.sqrt(dx * dx + dy * dy);
        const light = Math.max(0, 1 - distance / 220);
        const alpha = 0.05 + Math.max(0, wave) * 0.16 + light * 0.6;
        ctx!.fillStyle = `rgba(${light > 0 ? "120, 210, 255" : "110, 140, 220"}, ${alpha})`;
        ctx!.fillRect(x - 1, y - 1, 2, 2);
        if (light > 0.15) near.push([x, y, light]);
      }
    }

    ctx!.lineWidth = 1;
    for (let i = 0; i < near.length; i++) {
      for (let j = i + 1; j < near.length; j++) {
        const [x1, y1, l1] = near[i];
        const [x2, y2, l2] = near[j];
        if (Math.abs(x1 - x2) <= spacing && Math.abs(y1 - y2) <= spacing) {
          ctx!.strokeStyle = `rgba(56, 225, 255, ${Math.min(l1, l2) * 0.35})`;
          ctx!.beginPath();
          ctx!.moveTo(x1, y1);
          ctx!.lineTo(x2, y2);
          ctx!.stroke();
        }
      }
    }

    requestAnimationFrame(frame);
  }

  requestAnimationFrame(frame);
}
