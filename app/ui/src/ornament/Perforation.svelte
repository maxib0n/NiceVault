<!--
  The document number, laser-perforated through the card: a 5 x 7 dot-matrix of small holes,
  each a clean bore whose far wall catches the light and whose depth shows the dark beneath
  (on paper: the board below, a shade darker). The same number as the MRZ's first field.
  Fills its positioned parent, the row of holes centred in it.
-->
<script lang="ts" module>
  // 5 x 7 dot-matrix glyphs, one row per string, '#' = a hole
  const GLYPHS: Record<string, string[]> = {
    '0': ['.###.', '#...#', '#..##', '#.#.#', '##..#', '#...#', '.###.'],
    '1': ['..#..', '.##..', '..#..', '..#..', '..#..', '..#..', '.###.'],
    '2': ['.###.', '#...#', '....#', '...#.', '..#..', '.#...', '#####'],
    '3': ['#####', '...#.', '..#..', '...#.', '....#', '#...#', '.###.'],
    '4': ['...#.', '..##.', '.#.#.', '#..#.', '#####', '...#.', '...#.'],
    '5': ['#####', '#....', '####.', '....#', '....#', '#...#', '.###.'],
    '6': ['..##.', '.#...', '#....', '####.', '#...#', '#...#', '.###.'],
    '7': ['#####', '....#', '...#.', '..#..', '.#...', '.#...', '.#...'],
    '8': ['.###.', '#...#', '#...#', '.###.', '#...#', '#...#', '.###.'],
    '9': ['.###.', '#...#', '#...#', '.####', '....#', '...#.', '.##..'],
    A: ['.###.', '#...#', '#...#', '#####', '#...#', '#...#', '#...#'],
    B: ['####.', '#...#', '#...#', '####.', '#...#', '#...#', '####.'],
    C: ['.###.', '#...#', '#....', '#....', '#....', '#...#', '.###.'],
    D: ['####.', '#...#', '#...#', '#...#', '#...#', '#...#', '####.'],
    E: ['#####', '#....', '#....', '####.', '#....', '#....', '#####'],
    F: ['#####', '#....', '#....', '####.', '#....', '#....', '#....'],
    G: ['.###.', '#...#', '#....', '#.###', '#...#', '#...#', '.####'],
    H: ['#...#', '#...#', '#...#', '#####', '#...#', '#...#', '#...#'],
    I: ['.###.', '..#..', '..#..', '..#..', '..#..', '..#..', '.###.'],
    J: ['..###', '...#.', '...#.', '...#.', '...#.', '#..#.', '.##..'],
    K: ['#...#', '#..#.', '#.#..', '##...', '#.#..', '#..#.', '#...#'],
    L: ['#....', '#....', '#....', '#....', '#....', '#....', '#####'],
    M: ['#...#', '##.##', '#.#.#', '#.#.#', '#...#', '#...#', '#...#'],
    N: ['#...#', '#...#', '##..#', '#.#.#', '#..##', '#...#', '#...#'],
    O: ['.###.', '#...#', '#...#', '#...#', '#...#', '#...#', '.###.'],
    P: ['####.', '#...#', '#...#', '####.', '#....', '#....', '#....'],
    Q: ['.###.', '#...#', '#...#', '#...#', '#.#.#', '#..#.', '.##.#'],
    R: ['####.', '#...#', '#...#', '####.', '#.#..', '#..#.', '#...#'],
    S: ['.####', '#....', '#....', '.###.', '....#', '....#', '####.'],
    T: ['#####', '..#..', '..#..', '..#..', '..#..', '..#..', '..#..'],
    U: ['#...#', '#...#', '#...#', '#...#', '#...#', '#...#', '.###.'],
    V: ['#...#', '#...#', '#...#', '#...#', '#...#', '.#.#.', '..#..'],
    W: ['#...#', '#...#', '#...#', '#.#.#', '#.#.#', '#.#.#', '.#.#.'],
    X: ['#...#', '#...#', '.#.#.', '..#..', '.#.#.', '#...#', '#...#'],
    Y: ['#...#', '#...#', '.#.#.', '..#..', '..#..', '..#..', '..#..'],
    Z: ['#####', '....#', '...#.', '..#..', '.#...', '#....', '#####'],
  };

  /** Hole centres of `text` on a grid of `pitch` px, origin at the top-left of the row. */
  export function holes(text: string): { x: number; y: number }[] {
    const out: { x: number; y: number }[] = [];
    let col = 0;
    for (const ch of text.toUpperCase()) {
      const g = GLYPHS[ch];
      if (g) for (let r = 0; r < 7; r++) for (let c = 0; c < 5; c++) if (g[r][c] === '#') out.push({ x: col + c, y: r });
      col += 6;
    }
    return out;
  }
</script>

<script lang="ts">
  import InkCanvas, { type InkEnv } from './InkCanvas.svelte';

  let { text }: { text: string } = $props();

  function draw(ctx: CanvasRenderingContext2D, w: number, h: number, env: InkEnv) {
    const cols = Math.max(1, text.length * 6 - 1);
    // as large as the box allows, the holes on whole device pixels' worth of pitch
    const pitch = Math.max(2, Math.min((w * 0.94) / cols, h / 7.6));
    const r = pitch * 0.34;
    const x0 = (w - (cols - 1) * pitch) / 2;
    const y0 = (h - 6 * pitch) / 2;
    const pts = holes(text);
    const pressed = env.num('--ink-relief', 1) < 0;
    const dot = (dx: number, dy: number, rr: number, style: string) => {
      ctx.fillStyle = style;
      ctx.beginPath();
      for (const p of pts) {
        const x = x0 + p.x * pitch + dx;
        const y = y0 + p.y * pitch + dy;
        ctx.moveTo(x + rr, y);
        ctx.arc(x, y, rr, 0, Math.PI * 2);
      }
      ctx.fill();
    };
    // the bore's far (bottom-right) wall catches the light, its near wall is in shade
    dot(0.35, 0.4, r, env.ink('var(--relief-hi)'));
    dot(-0.25, -0.3, r, env.ink('var(--relief-lo)'));
    // what shows through: the dark beneath the card (anthracite) or the board (paper)
    dot(0, 0, r * 0.86, env.ink(pressed ? 'rgb(0 0 0 / 0.78)' : 'color-mix(in oklab, var(--ground-lo) 82%, var(--ink))'));
  }
</script>

<span class="perforation" aria-hidden="true"><InkCanvas class="perf-ink" {draw} /></span>

<style>
  .perforation {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
  }
  .perforation > :global(.perf-ink) {
    width: 100%;
    height: 100%;
  }
</style>
