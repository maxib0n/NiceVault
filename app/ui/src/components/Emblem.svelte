<!--
  The volume's emblem: the big mark on its document, printed, never pasted.
    initial  an engraved Newsreader monogram (hatched, outlined, raised in intaglio)
    icon     the lucide glyph cut as an engraving: a double hairline with hatching
    image    the user's picture as a security engraving (a line screen in the ink)
    hidden   opted out while locked: a neutral rosette, no initial
  finish: intaglio (ink on paper, the default), foil (stamped on the cover cloth),
  flat (tiny sizes). Hooks: .emblem[data-kind], [data-finish].
-->
<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import { iconLabel, imageSrc, initialOf, viewOf, type EmblemIconId, type EmblemView } from '../lib/emblem';
  import InkCanvas, { type InkEnv } from '../ornament/InkCanvas.svelte';
  import { toneMap, lineScreen, type ToneMap } from '../ornament/engrave';
  import { hair } from '../ornament/ink.svelte';
  import { circle, petals } from '../ornament/lathe';
  import { mask, printMask, release, type Finish } from '../ornament/print';
  import { EMBLEM_GLYPHS, glyphCentre, glyphPath } from '../ornament/glyphs';


  let {
    emblem,
    name,
    size = 48,
    decorative = false,
    finish = 'intaglio',
  }: {
    emblem: EmblemView | null | undefined;
    /** volume name (for the initial and the label) */
    name: string;
    size?: number;
    /** true when a visible label next to it already names the volume */
    decorative?: boolean;
    finish?: Finish;
  } = $props();

  const v = $derived(viewOf(emblem));
  const label = $derived(
    v.kind === 'hidden'
      ? m.emblem_hidden()
      : v.kind === 'icon'
        ? m.emblem_icon({ name: iconLabel(v.id) })
        : v.kind === 'image'
          ? m.emblem_image()
          : m.emblem_initial({ letter: initialOf(name) }),
  );

  // the source glyph of an icon emblem (read as geometry, never shown)
  let src: HTMLSpanElement | undefined = $state();
  // the decoded picture of an image emblem
  let tone = $state<ToneMap | null>(null);
  $effect(() => {
    const view = v;
    tone = null;
    if (view.kind !== 'image') return;
    let live = true;
    void toneMap(imageSrc(view.data_base64)).then((t) => {
      if (live) tone = t;
    });
    return () => {
      live = false;
    };
  });

  /** hatching: fine parallel lines over the whole mark, kept only where `keep` paints */
  function hatch(m: CanvasRenderingContext2D, w: number, h: number, gap: number, width: number, angle = 0) {
    m.save();
    m.lineWidth = width;
    m.translate(w / 2, h / 2);
    m.rotate(angle);
    const d = Math.hypot(w, h);
    m.beginPath();
    for (let y = -d / 2; y <= d / 2; y += gap) {
      m.moveTo(-d / 2, y);
      m.lineTo(d / 2, y);
    }
    m.stroke();
    m.restore();
  }

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const S = Math.min(w, h);
    const hw = hair(env.dpr);
    const fine = S >= 40 && finish !== 'flat';
    const view = v;
    let m: HTMLCanvasElement | null = null;

    if (view.kind === 'initial') {
      const letter = initialOf(name);
      const fs = S * 0.8;
      const font = `500 ${fs}px "Newsreader", Georgia, serif`;
      m = mask(env, w, h, (c) => {
        c.font = font;
        c.textAlign = 'center';
        c.textBaseline = 'alphabetic';
        const mt = c.measureText(letter);
        const asc = mt.actualBoundingBoxAscent || fs * 0.7;
        const desc = mt.actualBoundingBoxDescent || 0;
        const y = h / 2 + (asc - desc) / 2;
        // optical centre: the letter's ink box, not its advance width
        const x = w / 2 + (mt.actualBoundingBoxRight ? ((mt.actualBoundingBoxLeft || 0) - mt.actualBoundingBoxRight) / 2 : 0);
        if (fine && finish === 'intaglio') {
          // hatched body, cut by a hairline outline
          hatch(c, w, h, Math.max(1.3, S / 62), hw * 0.95);
          c.globalCompositeOperation = 'destination-in';
          c.fillText(letter, x, y);
          c.globalCompositeOperation = 'source-over';
          c.lineWidth = hw * 1.05;
          c.strokeText(letter, x, y);
        } else {
          c.fillText(letter, x, y);
        }
      });
    } else if (view.kind === 'icon') {
      const svg = src?.querySelector('svg');
      if (!svg) return;
      const geo = glyphPath(svg);
      const k = (S * (fine ? 0.64 : 0.7)) / 24;
      // optical centre: the glyph's own geometry, not its 24 grid, sits on the centre
      const gc = glyphCentre(svg);
      const ox = w / 2 - gc.x * k;
      const oy = h / 2 - gc.y * k;
      const outer = fine ? 2.5 : 2.1;
      m = mask(env, w, h, (c) => {
        c.save();
        c.translate(ox, oy);
        c.scale(k, k);
        c.lineWidth = outer;
        c.stroke(geo);
        if (fine && finish === 'intaglio') {
          // hollow the stroke to two hairlines, then hatch the channel between them
          const inner = outer - (2 * hw * 1.05) / k;
          c.globalCompositeOperation = 'destination-out';
          c.lineWidth = inner;
          c.stroke(geo);
          c.restore();
          const ch = mask(env, w, h, (cc) => {
            hatch(cc, w, h, Math.max(1.25, S / 64), hw * 0.9, -Math.PI / 4);
            cc.globalCompositeOperation = 'destination-in';
            cc.save();
            cc.translate(ox, oy);
            cc.scale(k, k);
            cc.lineWidth = inner;
            cc.stroke(geo);
            cc.restore();
          });
          c.globalCompositeOperation = 'source-over';
          c.drawImage(ch, 0, 0, w, h);
          release(ch);
        } else {
          c.restore();
        }
      });
    } else if (view.kind === 'image') {
      const t = tone;
      if (!t) return;
      const lines = Math.max(12, Math.round(S / (S >= 60 ? 2.7 : 2.4)));
      m = mask(env, w, h, (c) => {
        const r = S / 2 - 0.5;
        const p = new Path2D();
        lineScreen(p, t, w / 2 - r, h / 2 - r, r * 2, lines, hw * (S >= 60 ? 0.8 : 0.9));
        c.save();
        c.beginPath();
        c.arc(w / 2, h / 2, r - (fine ? 1.6 : 0.6), 0, Math.PI * 2);
        c.clip();
        c.fill(p);
        c.restore();
        if (fine) {
          c.lineWidth = hw;
          c.beginPath();
          c.arc(w / 2, h / 2, r, 0, Math.PI * 2);
          c.stroke();
        }
      });
    } else {
      // hidden: a neutral rosette, nothing that names the volume
      m = mask(env, w, h, (c) => {
        const R = S / 2 - 0.5;
        const p = new Path2D();
        circle(p, w / 2, h / 2, R * 0.97);
        petals(p, w / 2, h / 2, R, { r0: 0.42, r1: 0.9, lines: S >= 60 ? 4 : 3, k: S >= 60 ? 10 : 8, lean: 1 });
        if (S >= 60) petals(p, w / 2, h / 2, R, { r0: 0.12, r1: 0.4, lines: 3, k: 6, lean: 0.8 });
        circle(p, w / 2, h / 2, R * 0.42);
        c.lineWidth = hw * (S >= 60 ? 0.9 : 1.1);
        c.stroke(p);
      });
    }
    // a line screen is too dense for relief: it prints flat; everything else stands in intaglio
    if (m) printMask(ctx, m, env, w, h, view.kind === 'image' && finish === 'intaglio' ? 'flat' : finish);
  }
</script>

<span
  class="emblem"
  data-kind={v.kind}
  data-finish={finish}
  style:--emblem-size="{size}px"
  role={decorative ? undefined : 'img'}
  aria-label={decorative ? undefined : label}
  aria-hidden={decorative ? 'true' : undefined}
>
  <InkCanvas class="emblem-ink" {draw} />
  {#if v.kind === 'icon'}
    {@const Icon = EMBLEM_GLYPHS[v.id as EmblemIconId]}
    <span class="src" bind:this={src} aria-hidden="true"><Icon size={24} aria-hidden="true" /></span>
  {/if}
</span>

<style>
  .emblem {
    position: relative;
    display: inline-block;
    flex: none;
    width: var(--emblem-size);
    height: var(--emblem-size);
  }
  .emblem > :global(.emblem-ink) {
    width: 100%;
    height: 100%;
  }
  /* rendered but invisible: its geometry is measured (glyphCentre) */
  .src {
    position: absolute;
    left: 0;
    top: 0;
    width: 24px;
    height: 24px;
    overflow: hidden;
    visibility: hidden;
    pointer-events: none;
  }
</style>
