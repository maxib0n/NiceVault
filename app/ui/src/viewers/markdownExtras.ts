import { sanitizeMath, sanitizeSvg } from './markdown';

export async function renderMath(root: HTMLElement): Promise<number> {
  const els = [...root.querySelectorAll<HTMLElement>('.md-math:not([data-rendered])')];
  if (!els.length) return 0;
  const [{ default: katex }] = await Promise.all([import('katex'), import('katex/dist/katex.min.css')]);
  let n = 0;
  for (const el of els) {
    const tex = el.textContent ?? '';
    try {
      const html = katex.renderToString(tex, {
        displayMode: el.dataset.display === '1',
        throwOnError: false,
        trust: false,
        strict: 'ignore',
        output: 'htmlAndMathml',
        maxSize: 40,
        maxExpand: 400,
      });
      el.innerHTML = sanitizeMath(html);
      el.title = tex;
      el.dataset.rendered = '1';
      n++;
    } catch {
      el.dataset.rendered = 'error';
    }
  }
  return n;
}

const sources = new WeakMap<HTMLElement, string>();
let seq = 0;

function hex(root: HTMLElement, expr: string): string {
  const probe = document.createElement('span');
  probe.style.color = expr;
  probe.style.display = 'none';
  root.appendChild(probe);
  const c = getComputedStyle(probe).color;
  probe.remove();
  const cv = document.createElement('canvas');
  cv.width = cv.height = 1;
  const ctx = cv.getContext('2d', { willReadFrequently: true });
  if (!ctx) return '#888888';
  ctx.fillStyle = '#888888';
  ctx.fillStyle = c;
  ctx.fillRect(0, 0, 1, 1);
  const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
  return '#' + [r, g, b].map((x) => x.toString(16).padStart(2, '0')).join('');
}

const luminance = (h: string) => {
  const n = parseInt(h.slice(1), 16);
  return (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255;
};

export function mermaidTheme(root: HTMLElement): Record<string, string | boolean> {
  const paper = hex(root, 'var(--paper-hi)');
  const dark = luminance(paper) < 0.45;
  const ink = hex(root, 'var(--ink)');
  const ink2 = hex(root, 'var(--ink-2)');
  const node = hex(root, `color-mix(in srgb, var(--accent) ${dark ? 22 : 10}%, var(--paper-hi))`);
  const node2 = hex(root, `color-mix(in srgb, var(--accent) ${dark ? 12 : 5}%, var(--paper-sunk, var(--paper-hi)))`);
  const line = hex(root, `color-mix(in srgb, var(--accent) 45%, var(--ink-3, var(--ink-2)))`);
  const border = hex(root, `color-mix(in srgb, var(--accent) 70%, var(--paper-hi))`);
  const font = getComputedStyle(root).getPropertyValue('--font-ui').trim() || 'sans-serif';
  const scale: Record<string, string> = {};
  const steps = dark ? [34, 22, 46, 16, 28, 40, 20, 52, 26, 38, 18, 44] : [22, 12, 32, 8, 18, 28, 10, 38, 15, 25, 6, 35];
  steps.forEach((p, i) => {
    const c = hex(root, `color-mix(in srgb, var(--accent) ${p}%, ${i % 2 ? 'var(--paper-sunk, var(--paper-hi))' : 'var(--paper-hi)'})`);
    scale[`cScale${i}`] = c;
    scale[`cScaleLabel${i}`] = ink;
    scale[`cScalePeer${i}`] = border;
    if (i < 8) {
      scale[`fillType${i}`] = c;
      scale[`git${i}`] = hex(root, `color-mix(in srgb, var(--accent) ${100 - i * 9}%, var(--ink-3, var(--ink-2)))`);
      scale[`gitBranchLabel${i}`] = dark ? ink : paper;
    }
  });
  const pie = [100, 62, 82, 55, 92, 70, 58, 88, 66, 76, 60, 85];
  pie.forEach((p, i) => (scale[`pie${i + 1}`] = hex(root, `color-mix(in srgb, var(--accent) ${p}%, var(--paper-hi))`)));
  return {
    ...scale,
    pieOpacity: '1',
    pieStrokeColor: paper,
    pieOuterStrokeColor: border,
    pieTitleTextColor: ink,
    pieSectionTextColor: dark ? ink : paper,
    pieLegendTextColor: ink,
    darkMode: dark,
    background: paper,
    fontFamily: font,
    fontSize: '14px',
    primaryColor: node,
    primaryBorderColor: border,
    primaryTextColor: ink,
    secondaryColor: node2,
    secondaryBorderColor: border,
    secondaryTextColor: ink,
    tertiaryColor: paper,
    tertiaryBorderColor: border,
    tertiaryTextColor: ink,
    mainBkg: node,
    nodeBorder: border,
    nodeTextColor: ink,
    textColor: ink,
    titleColor: ink,
    lineColor: line,
    edgeLabelBackground: paper,
    clusterBkg: node2,
    clusterBorder: border,
    noteBkgColor: node2,
    noteBorderColor: border,
    noteTextColor: ink2,
    actorBkg: node,
    actorBorder: border,
    actorTextColor: ink,
    signalColor: ink2,
    signalTextColor: ink,
    labelBoxBkgColor: node,
    labelBoxBorderColor: border,
    labelTextColor: ink,
  };
}

export async function renderMermaid(root: HTMLElement): Promise<'ready' | 'error' | 'none'> {
  const blocks = [...root.querySelectorAll<HTMLElement>('.md-mermaid')];
  if (!blocks.length) return 'none';
  const { default: mermaid } = await import('mermaid');
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: 'strict',
    theme: 'base',
    themeVariables: mermaidTheme(root),
    htmlLabels: false,
    suppressErrorRendering: true,
    maxTextSize: 50_000,
    maxEdges: 500,
    look: 'classic',
  });
  let failed = false;
  for (const b of blocks) {
    let text = sources.get(b);
    if (text === undefined) {
      text = b.querySelector('pre')?.textContent ?? '';
      sources.set(b, text);
    }
    try {
      const { svg } = await mermaid.render(`nv-mmd-${++seq}`, text);
      const clean = sanitizeSvg(svg);
      if (!clean.includes('<svg')) throw new Error('empty');
      b.innerHTML = clean;
      b.dataset.state = 'ready';
    } catch (e) {
      failed = true;
      b.dataset.error = String((e as Error)?.message ?? e).slice(0, 300);
      if (!b.querySelector('pre')) {
        const pre = document.createElement('pre');
        pre.className = 'md-code';
        pre.dataset.lang = 'mermaid';
        const code = document.createElement('code');
        code.textContent = text;
        pre.appendChild(code);
        b.replaceChildren(pre);
      }
      b.dataset.state = 'error';
    } finally {
      document.getElementById(`dnv-mmd-${seq}`)?.remove();
    }
  }
  return failed ? 'error' : 'ready';
}
