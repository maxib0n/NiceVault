#!/usr/bin/env python3
"""Classifies captured X frames (reveal.mjs) in one screen region, against the settled frame.

  frames.py <frames dir> <x> <y> <w> <h> [arrive]

Per frame: "-" no content there yet (flat: the window not mapped, the ground, a held view),
"F" the settled content already shown plainly, "G" anything else (glyphs, the decode under
way). A plain-text flash is an F before a G: the settled content shown, then animated; with
"arrive" (a view that appears from nothing) also content, then nothing, then content again: the
plain view shown, then held for its decode (its text may differ from the settled one). Prints one JSON line and writes
<dir>/strip.png (the region of every frame, side by side, labelled) and <dir>/final-region.png.
"""
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

d, x, y, w, h = sys.argv[1], *map(int, sys.argv[2:6])
arrive = len(sys.argv) > 6 and sys.argv[6] == 'arrive'
names = sorted(f for f in os.listdir(d) if f.startswith('f') and f.endswith('.png'))
crops = []
for n in names:
    try:
        im = Image.open(os.path.join(d, n)).convert('L')
    except Exception:
        continue  # a frame cut short when the capture was stopped
    crops.append((n, np.asarray(im.crop((x, y, x + w, y + h)), dtype=np.float32)))
final = crops[-1][1]
# the pixels that carry the content in the settled frame: text and edges (far from the median)
ink = np.abs(final - np.median(final)) > 28
labels = []
for n, c in crops:
    flat = np.abs(c - np.median(c)) > 28
    if flat.mean() < 0.25 * ink.mean():
        labels.append('-')
        continue
    same = np.mean(np.abs(c - final)[ink | flat] < 18)
    labels.append('F' if same > 0.9 else 'G')
s = ''.join(labels)
g = s.find('G')
f = s.find('F')
import re  # noqa: E402

flash = (f >= 0 and 'G' in s[f:]) or (arrive and re.search(r'[GF]-+[GF]', s) is not None)
# a strip for the eye: every frame's region, scaled down
sc = min(1.0, 220 / max(w, 1))
tw, th = max(1, int(w * sc)), max(1, int(h * sc))
cols = 10
rows = (len(crops) + cols - 1) // cols
strip = Image.new('L', (cols * tw, rows * (th + 14)), 0)
dr = ImageDraw.Draw(strip)
for i, (n, c) in enumerate(crops):
    im = Image.fromarray(c.astype(np.uint8)).resize((tw, th))
    px, py = (i % cols) * tw, (i // cols) * (th + 14)
    strip.paste(im, (px, py + 14))
    dr.text((px + 2, py + 1), f'{n[1:-4]} {labels[i]}', fill=255)
strip.save(os.path.join(d, 'strip.png'))
Image.fromarray(final.astype(np.uint8)).save(os.path.join(d, 'final-region.png'))
print(json.dumps({'frames': len(crops), 'labels': s, 'first_glyph': g, 'first_final': f, 'flash': flash}))
