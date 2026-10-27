#!/usr/bin/env python3
"""Material tiles for the UI (CC0, drawn for NiceVault). Seeded and periodic by
construction (all noise is synthesised in the frequency domain on a torus), so the
tiles repeat without a seam and rebuild identically.

  suede-nap.webp   anthracite Alcantara: the nap of a microfibre suede, 512 px.
                   Only fine scales survive (a band-pass between ~1 and ~5 px): no
                   cloud, no blotch, nothing large enough to be seen repeating.
                   Fibres lean one way (towards the bottom-right, away from the
                   top-left light), so the grain is short and slightly directional.
  paper-grain.webp cotton paper for the light theme: a fine grain and a few short
                   fibres, high-passed so the sheet stays even (no fog).
  felt-board.webp  the board the documents lie on (dark theme): a pressed wool felt seen at
                   arm's length, micro fibres (2-6 px) crossing every way, packed so densely
                   that no clump survives (an earlier felt, 5-15 px fibres and a strong
                   print, read as noise). Tight and even: everything coarser
                   than ~5 px is high-passed out, and its spread is kept small. No nap, no lean:
                   of another character than the suede of the covers, so a cover never melts
                   into the board it lies on.
  board-grain.webp the board in the light theme: the cotton paper's grain alone, finer (1-2 px)
                   and without its long fibres; the sheets keep paper-grain.webp.
  *-aNN.webp       the printing copies (styles/tokens.css --*-print): the same tile with its
                   strength (NN %, times --tex-k per resolution) baked into its alpha, painted as
                   the surface's own top background layer with background-blend-mode (base.css).

  dither.png       64 px blue noise (void-and-cluster), mid-grey, soft-light blended
                   under the light pools (board, card, cover): a dark gradient's 8-bit
                   steps fall at a different place in every pixel (+-2/255), so a long,
                   quiet gradient shows no bands and no grain (1 texel = 1 device px).

The material tiles are mid-grey centred (128), blended in CSS (soft-light / overlay).
Usage: python3 scripts/gen-textures.py  (writes src/assets/textures/)
Needs numpy and Pillow.
"""
import os
import numpy as np
from PIL import Image

OUT = os.path.join(os.path.dirname(__file__), '..', 'src', 'assets', 'textures')
N = 512


def spectrum_noise(rng, n, band_lo, band_hi, aniso=1.0, angle_deg=0.0):
    """White noise filtered in the frequency domain: keeps periods between band_hi..band_lo px.
    aniso > 1 stretches the grain along `angle_deg` (fibres lying one way)."""
    white = rng.standard_normal((n, n))
    F = np.fft.fft2(white)
    fy = np.fft.fftfreq(n)[:, None]
    fx = np.fft.fftfreq(n)[None, :]
    a = np.deg2rad(angle_deg)
    u = fx * np.cos(a) + fy * np.sin(a)
    v = -fx * np.sin(a) + fy * np.cos(a)
    r = np.sqrt((u * aniso) ** 2 + (v / aniso) ** 2)
    lo, hi = 1.0 / band_lo, 1.0 / band_hi  # cycles per px
    def edge(x, a0, a1):
        t = np.clip((x - a0) / (a1 - a0), 0, 1)
        return 0.5 - 0.5 * np.cos(np.pi * t)
    band = edge(r, lo * 0.7, lo * 1.3) * (1 - edge(r, hi * 0.75, hi * 1.0))
    out = np.real(np.fft.ifft2(F * band))
    return out / (out.std() + 1e-9)


def periodic_strokes(rng, n, count, length, width, tone_sd):
    """Short fibres on a torus, drawn as soft anti-aliased segments (accumulated in float)."""
    acc = np.zeros((n, n))
    yy, xx = np.mgrid[0:n, 0:n]
    for _ in range(count):
        x0, y0 = rng.uniform(0, n, 2)
        ang = rng.uniform(0, np.pi)
        L = length * rng.uniform(0.6, 1.4)
        curve = rng.normal(0, 0.25)
        tone = rng.normal(0, tone_sd)
        steps = int(L * 1.5) + 2
        for s in range(steps):
            t = s / (steps - 1) - 0.5
            a2 = ang + curve * t
            px = (x0 + np.cos(a2) * L * t) % n
            py = (y0 + np.sin(a2) * L * t) % n
            ix, iy = int(px), int(py)
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    w = np.exp(-((ix + dx - px) ** 2 + (iy + dy - py) ** 2) / (2 * width ** 2))
                    acc[(iy + dy) % n, (ix + dx) % n] += tone * w / steps * 6
    return acc


def highpass(img, k=24):
    """Remove everything coarser than k px (periodic box blur subtracted)."""
    F = np.fft.fft2(img)
    fy = np.fft.fftfreq(img.shape[0])[:, None]
    fx = np.fft.fftfreq(img.shape[1])[None, :]
    r = np.sqrt(fx ** 2 + fy ** 2)
    keep = 1 - np.exp(-((r * k) ** 2) * 2)
    return np.real(np.fft.ifft2(F * keep))


def save(name, field, sd):
    img = np.clip(128 + field * sd, 0, 255).astype(np.uint8)
    rgb = np.stack([img] * 3, axis=-1)
    Image.fromarray(rgb, 'RGB').save(os.path.join(OUT, name), 'WEBP', quality=90, method=6)
    print(name, 'std', round(float(img.std()), 2), 'mean', round(float(img.mean()), 2))


def suede():
    rng = np.random.default_rng(20261024)
    fine = spectrum_noise(rng, N, 3.0, 1.15, aniso=2.4, angle_deg=-35)
    tuft = spectrum_noise(rng, N, 5.0, 2.4, aniso=1.6, angle_deg=-35)
    nap = 0.8 * fine + 0.2 * tuft
    nap = np.tanh(nap * 0.9) / 0.9
    tips = (rng.random((N, N)) < 0.006) * rng.uniform(0.8, 2.2, (N, N))
    nap = nap + tips
    nap = highpass(nap, 14)
    save('suede-nap.webp', nap / nap.std(), 22)


def paper():
    rng = np.random.default_rng(1985)
    grain = spectrum_noise(rng, N, 2.6, 1.1, aniso=1.0)
    fibres = periodic_strokes(rng, N, 900, 18, 0.7, 1.0)
    p = 0.85 * grain + 0.9 * fibres / (fibres.std() + 1e-9)
    p = highpass(p, 20)
    save('paper-grain.webp', p / p.std(), 20)


def felt():
    from PIL import ImageDraw
    rng = np.random.default_rng(1924)
    S = 4  # drawn 4x larger, then reduced: anti-aliased fibres
    big = N * S
    layers = []
    for count, tone in ((60000, 255), (60000, 255)):
        im = Image.new('L', (big, big), 0)
        dr = ImageDraw.Draw(im)
        for _ in range(count):
            x0, y0 = rng.uniform(0, big, 2)
            ang = rng.uniform(0, np.pi)
            L = rng.uniform(2, 6) * S
            bend = rng.normal(0, 0.35)
            pts = []
            for t in np.linspace(-0.5, 0.5, 4):
                a2 = ang + bend * t
                pts.append((x0 + np.cos(a2) * L * t, y0 + np.sin(a2) * L * t))
            w = max(1, int(round(rng.uniform(0.35, 0.6) * S)))
            for ox in (-big, 0, big):
                for oy in (-big, 0, big):
                    q = [(x + ox, y + oy) for x, y in pts]
                    if all(x < 0 for x, _ in q) or all(x > big for x, _ in q) or all(y < 0 for _, y in q) or all(y > big for _, y in q):
                        continue
                    dr.line(q, fill=tone, width=w)
        layers.append(np.asarray(im.resize((N, N), Image.LANCZOS), dtype=float) / 255)
    fibres = layers[0] - layers[1]
    grain = spectrum_noise(rng, N, 2.0, 1.05)
    f = 1.0 * fibres / (fibres.std() + 1e-9) + 0.5 * grain
    f = np.tanh(f * 0.8) / 0.8
    f = highpass(f, 5)
    save('felt-board.webp', f / f.std(), 12)


def board_paper():
    rng = np.random.default_rng(1986)
    g = spectrum_noise(rng, N, 2.0, 1.05)
    g = np.tanh(g * 0.8) / 0.8
    g = highpass(g, 6)
    save('board-grain.webp', g / g.std(), 14)


def bake(name, alpha):
    """A printing copy: the tile with its strength (an opacity) as its alpha."""
    im = Image.open(os.path.join(OUT, name)).convert('RGBA')
    a = np.asarray(im).copy()
    a[..., 3] = round(alpha * 255)
    out = name.replace('.webp', f'-a{round(alpha * 100)}.webp')
    Image.fromarray(a, 'RGBA').save(os.path.join(OUT, out), 'WEBP', lossless=True)
    print(out)


PRINTS = (('felt-board.webp', 0.62), ('board-grain.webp', 0.2), ('paper-grain.webp', 0.26), ('suede-nap.webp', 0.16), ('suede-nap.webp', 1))
TEX_K = (0.6, 0.7, 0.8, 1)


def baked():
    for name, alpha in PRINTS:
        for k in TEX_K:
            if alpha * k < 1:
                bake(name, alpha * k)
    bake('cloth-weave.webp', 0.42)


def blue_noise(n=64, sigma=1.9, seed=7):
    """Ulichney's void-and-cluster on a torus: a rank 0..n*n-1 per texel, blue-noise ordered."""
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:n, 0:n]
    d = lambda a: np.minimum(a, n - a)
    kern = np.exp(-(d(x) ** 2 + d(y) ** 2) / (2 * sigma ** 2))
    energy = lambda m: np.real(np.fft.ifft2(np.fft.fft2(m) * np.fft.fft2(kern)))
    shift = lambda i: np.roll(np.roll(kern, i // n, 0), i % n, 1)
    pat = np.zeros(n * n, bool)
    pat[rng.choice(n * n, n * n // 10, replace=False)] = True
    e = energy(pat.reshape(n, n).astype(float)).ravel()
    while True:
        c = np.argmax(np.where(pat, e, -np.inf))
        pat[c] = False
        e -= shift(c).ravel()
        v = np.argmin(np.where(pat, np.inf, e))
        pat[v] = True
        e += shift(v).ravel()
        if v == c:
            break
    rank = np.zeros(n * n, int)
    ones = int(pat.sum())
    p1, e1 = pat.copy(), e.copy()
    for r in range(ones - 1, -1, -1):
        c = np.argmax(np.where(p1, e1, -np.inf))
        p1[c] = False
        e1 -= shift(c).ravel()
        rank[c] = r
    for r in range(ones, n * n):
        v = np.argmin(np.where(pat, np.inf, e))
        pat[v] = True
        e += shift(v).ravel()
        rank[v] = r
    return rank.reshape(n, n)


def dither():
    n = 64
    d = (blue_noise(n) + 0.5) / (n * n) - 0.5  # -0.5..0.5, uniform, blue
    g = np.rint(128 + np.where(d > 0, d * 18, d * 38)).astype(np.uint8)  # 109..137
    Image.fromarray(g, 'L').save(os.path.join(OUT, 'dither.png'), optimize=True)


if __name__ == '__main__':
    import sys
    only = set(sys.argv[1:])
    for name, fn in (('suede', suede), ('paper', paper), ('felt', felt), ('board', board_paper), ('dither', dither), ('baked', baked)):
        if not only or name in only:
            fn()
