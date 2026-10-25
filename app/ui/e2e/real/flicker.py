#!/usr/bin/env python3
"""Classifies the captured X frames (flicker.mjs) of one screen region: how a surface arrives.

  flicker.py <frames dir> <x> <y> <w> <h>

Per frame, against the frame before the open ("b", what was there) and the settled frame ("F"):
"b" still the old content, "F" settled, "~" in between (the fade / the decode), "!" a frame far
from both neighbours where they agree with each other (a flash: one frame darker, lighter or
emptier than the frames around it). A flicker is any "!", or the settled look shown and then
lost again (F then b / ~ then F). Prints one JSON line and writes <dir>/strip.png.
"""
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

d, x, y, w, h = sys.argv[1], *map(int, sys.argv[2:6])
# frames captured at a fraction of the screen (capture-kwin.py)
if os.path.exists(os.path.join(d, 'scale.txt')):
    k = int(open(os.path.join(d, 'scale.txt')).read())
    x, y, w, h = x // k, y // k, max(2, w // k), max(2, h // k)
names = sorted(f for f in os.listdir(d) if f.startswith('f') and f.endswith('.png'))
crops = []
for n in names:
    try:
        im = Image.open(os.path.join(d, n)).convert('L')
    except Exception:
        continue
    crops.append((n, np.asarray(im.crop((x, y, x + w, y + h)), dtype=np.float32)))
before, final = crops[0][1], crops[-1][1]
span = max(4.0, float(np.mean(np.abs(final - before))))


def dist(a, b):
    return float(np.mean(np.abs(a - b)))


labels, means = [], []
for i, (n, c) in enumerate(crops):
    db, df = dist(c, before), dist(c, final)
    lab = 'b' if db < 0.15 * span else 'F' if df < 0.15 * span else '~'
    if 0 < i < len(crops) - 1:
        p, q = crops[i - 1][1], crops[i + 1][1]
        around = dist(p, q)
        if dist(c, p) > 0.35 * span + 2 and dist(c, q) > 0.35 * span + 2 and around < 0.5 * min(dist(c, p), dist(c, q)):
            lab = '!'
    labels.append(lab)
    means.append(round(float(c.mean()), 1))
s = ''.join(labels)
f = s.find('F')
lost = f >= 0 and any(ch in 'b~' for ch in s[f:].rstrip('F')) and 'F' in s[f:].lstrip('F')
flicker = '!' in s or lost
sc = min(1.0, 200 / max(w, 1))
tw, th = max(1, int(w * sc)), max(1, int(h * sc))
cols = 12
rows = (len(crops) + cols - 1) // cols
strip = Image.new('L', (cols * tw, rows * (th + 14)), 0)
dr = ImageDraw.Draw(strip)
for i, (n, c) in enumerate(crops):
    im = Image.fromarray(c.astype(np.uint8)).resize((tw, th))
    px, py = (i % cols) * tw, (i // cols) * (th + 14)
    strip.paste(im, (px, py + 14))
    dr.text((px + 2, py + 1), f'{n[1:-4]} {labels[i]}', fill=255)
strip.save(os.path.join(d, 'strip.png'))
print(json.dumps({'frames': len(crops), 'labels': s, 'flicker': flicker, 'means': means}))
