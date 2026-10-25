// The hero emblem: ciphertext is a field of random hex, and what is inside only shows
// where a key is applied. A lens (your pointer, or a slow idle path) turns the hex under
// it into the names of files. It pauses off-screen and in a hidden tab; with
// prefers-reduced-motion it draws one still frame. (Text and picture decodes: reveal.js.)
(() => {
  'use strict';

  const reduceMQ = matchMedia('(prefers-reduced-motion: reduce)');
  const HEX = '0123456789abcdef';
  const FILES = 'contracts.pdf   scan-0042.jpg   recovery-codes.txt   tax-2025.xlsx   passport.png   notes.md   keys.kdbx   ';
  const DENSE = '06898bde';
  const GOLD = ['#fff1cb', '#f0d9a0', '#dfc07c', '#c49f58'];
  const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
  const smooth = (a, b, v) => { const t = clamp((v - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };
  const rnd = () => HEX[(Math.random() * 16) | 0];

  function fontReady() {
    return document.fonts && document.fonts.load
      ? Promise.all([document.fonts.load('500 12px "IBM Plex Mono"'), document.fonts.load('400 12px "IBM Plex Mono"')]).catch(() => {})
      : Promise.resolve();
  }

  // ------------------------------------------------------------------ hero
  function hero(canvas) {
    const host = canvas.parentElement;
    const ctx = canvas.getContext('2d');

    let W = 0, H = 0, dpr = 1, cols = 0, rows = 0, cw = 0, ch = 0, fs = 0, gw = 0, gh = 0, ox = 0, oy = 0;
    let glyph, hash, ink, body;
    let running = false, raf = 0, last = 0, start = 0;
    const lens = { x: 0, y: 0, r: 0, tx: 0, ty: 0, tr: 0, user: false, moved: -1e9 };
    let still = reduceMQ.matches;

    function build() {
      W = host.clientWidth; H = host.clientHeight;
      if (!W || !H) return;
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr);
      fs = clamp(W / 54, 8, 12);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.font = `500 ${fs}px "IBM Plex Mono", ui-monospace, monospace`;
      cw = ctx.measureText('0').width || fs * 0.6;
      ch = fs * 1.34;
      cols = Math.floor(W / cw); rows = Math.floor(H / ch);
      gw = cols * cw; gh = rows * ch; ox = (W - gw) / 2; oy = (H - gh) / 2;
      const n = cols * rows;
      glyph = new Uint8Array(n); hash = new Float32Array(n); ink = new Float32Array(n); body = new Float32Array(n);
      for (let i = 0; i < n; i++) { glyph[i] = (Math.random() * 16) | 0; hash[i] = Math.random(); }
      lens.x = lens.tx = gw / 2; lens.y = lens.ty = gh / 2; lens.r = lens.tr = Math.min(gw, gh) * 0.2;
      start = performance.now();
      if (still) frame(start + 5000, true);
    }

    // the seal, per cell: distance from the cell centre to rings and guilloche bands
    function field(t) {
      const R = Math.min(gw, gh) / 2;
      const cx = gw / 2, cy = gh / 2;
      const w = Math.max(8, ch * 0.72);
      const line = (d, k) => 1 - smooth(w * k * 0.55, w * k, d);
      for (let r = 0; r < rows; r++) {
        const dy = r * ch + ch / 2 - cy;
        for (let c = 0; c < cols; c++) {
          const dx = c * cw + cw / 2 - cx;
          const rad = Math.hypot(dx, dy) / R, ang = Math.atan2(dy, dx);
          let v = 0;
          v = Math.max(v, line(Math.abs(rad - 0.92) * R, 1), line(Math.abs(rad - 0.655) * R, 1), line(Math.abs(rad - 0.34) * R, 1));
          if (rad > 0.6 && rad < 0.9) {
            v = Math.max(v,
              line(Math.abs(rad - 0.775 - 0.05 * Math.cos(40 * ang + t * 0.0012)) * R, 0.75),
              line(Math.abs(rad - 0.775 - 0.05 * Math.cos(40 * ang + Math.PI - t * 0.0012)) * R, 0.75));
          } else if (rad > 0.36 && rad < 0.64) {
            v = Math.max(v, line(Math.abs(rad - 0.5 - 0.045 * Math.cos(28 * ang - t * 0.0016)) * R, 0.75));
          }
          const i = r * cols + c;
          ink[i] = v;
          body[i] = rad > 0.34 && rad < 0.92 ? 0.055 : rad <= 0.34 ? 0.07 : 0;
        }
      }
    }

    function frame(now, once) {
      if (!cols) return;
      const t = now - start;
      const dt = Math.min(64, now - (frame.prev || now)); frame.prev = now;
      const R = Math.min(gw, gh) / 2;
      field(once ? 0 : t);

      // the lens
      if (once) { lens.x = gw / 2; lens.y = gh / 2; lens.r = R * 0.22; }
      else {
        const idle = !lens.user || now - lens.moved > 1800;
        if (idle) {
          lens.tx = gw / 2 + R * 0.3 * Math.sin(t / 2600);
          lens.ty = gh / 2 + R * 0.26 * Math.sin(t / 1900 + 1.2);
          lens.tr = R * (0.17 + 0.012 * Math.sin(t / 900));
          if (lens.user && now - lens.moved > 1800) lens.user = false;
        }
        const k = 1 - Math.exp(-dt / 140);
        lens.x += (lens.tx - lens.x) * k; lens.y += (lens.ty - lens.y) * k; lens.r += (lens.tr - lens.r) * k;
      }

      const intro = once ? 2 : clamp(t / 2000, 0, 2);
      const maxD = Math.hypot(gw, gh) / 2;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, W, H);
      ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.font = `500 ${fs}px "IBM Plex Mono", ui-monospace, monospace`;
      const L = FILES.length;
      let fill = '';
      for (let r = 0; r < rows; r++) {
        for (let c = 0; c < cols; c++) {
          const i = r * cols + c;
          const px = c * cw + cw / 2, py = r * ch + ch / 2;
          const cd = Math.hypot(px - gw / 2, py - gh / 2) / maxD;
          const settled = cd < intro * 0.9;
          const a = ink[i];
          // glyph churn: fast while unresolved, slow shimmer once settled
          if (!once && Math.random() < (settled ? (a > 0.1 ? 0.018 : 0.006) : 0.45)) glyph[i] = (Math.random() * 16) | 0;
          const lf = 1 - smooth(lens.r * 0.62, lens.r, Math.hypot(px - lens.x, py - lens.y));
          let ch1 = a > 0.5 && settled ? DENSE[glyph[i] & 7] : HEX[glyph[i]], col, alpha;
          if (lf > hash[i] * 0.98 && lf > 0.02 && settled) {
            ch1 = FILES[(c + r * 13) % L];
            if (ch1 === ' ') continue;
            col = '#ffe8e2'; alpha = 0.55 + 0.45 * lf;
          } else if (!settled) {
            col = '#a5a39e'; alpha = 0.22 + Math.random() * 0.2;
          } else if (a > 0.04) {
            col = GOLD[Math.min(3, ((c / cols + r / rows) * 2.1) | 0)];
            alpha = 0.35 + 0.65 * a;
          } else {
            col = '#b9a9a3'; alpha = 0.04 + body[i] * 1.2 + 0.025 * Math.sin(t / 1400 + i * 0.37);
            if (once) alpha = 0.05 + body[i] * 1.2;
          }
          if (col !== fill) { ctx.fillStyle = col; fill = col; }
          ctx.globalAlpha = alpha;
          ctx.fillText(ch1, ox + px, oy + py);
        }
      }
      ctx.globalAlpha = 1;
      // the lens edge, drawn once as a hairline
      if (intro > 1.2) {
        ctx.strokeStyle = 'rgba(242,104,115,0.32)'; ctx.lineWidth = 1;
        ctx.beginPath(); ctx.arc(ox + lens.x, oy + lens.y, lens.r * 1.02, 0, 6.2832); ctx.stroke();
      }
    }

    function loop(now) {
      if (!running) return;
      raf = requestAnimationFrame(loop);
      if (now - last < 33) return;
      last = now;
      frame(now);
    }
    function run(v) {
      if (still) return;
      if (v && !running) { running = true; last = 0; frame.prev = 0; raf = requestAnimationFrame(loop); }
      else if (!v && running) { running = false; cancelAnimationFrame(raf); }
    }

    let vis = true;
    const sync = () => run(vis && !document.hidden);
    new IntersectionObserver((es) => { vis = es[0].isIntersecting; sync(); }, { threshold: 0.05 }).observe(host);
    document.addEventListener('visibilitychange', sync);

    host.addEventListener('pointermove', (e) => {
      if (still) return;
      const b = host.getBoundingClientRect();
      lens.tx = e.clientX - b.left - ox; lens.ty = e.clientY - b.top - oy;
      lens.tr = Math.min(gw, gh) * 0.2;
      lens.user = true; lens.moved = performance.now();
    });
    host.addEventListener('pointerleave', () => { lens.moved = performance.now() - 900; });

    let rt = 0;
    new ResizeObserver(() => { clearTimeout(rt); rt = setTimeout(() => { build(); }, 80); }).observe(host);
    reduceMQ.addEventListener('change', () => location.reload());
    build();
    sync();
  }

  fontReady().then(() => {
    const c = document.getElementById('stage-canvas');
    if (c) hero(c);
  });
})();
