#!/usr/bin/env python3
"""How a decode ends, per item, in captured X frames (reveal.mjs, capture.py).

  decode-end.py <frames dir> <boxes.json>

boxes.json: [[x, y, w, h], ...] in screen px (an item's picture, or its whole tile). Per box, every
frame is scored against the settled (last) frame: the share of the pixels that carry content there
(or in that frame) within 18 grey levels of it. "settle" is the first frame from which the box stays
settled (>= 0.97); "last" is the score of the frame just before it: what the decode showed right
before it handed over. A decode that plays to its end converges (last close to 1); one cut short
jumps from glyphs to the content (last well below). "glyphs" says whether the box ever showed
something else than its settled content (the decode was seen at all).
Prints one JSON line and writes <dir>/end-<i>.png: the box's last 8 frames before settling, then the
settled one, side by side (scaled x3).
"""
import json
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

d, boxes = sys.argv[1], json.load(open(sys.argv[2]))
names = sorted(f for f in os.listdir(d) if f.startswith('f') and f.endswith('.png'))
frames = []
for n in names:
    try:
        frames.append(np.asarray(Image.open(os.path.join(d, n)).convert('L'), dtype=np.float32))
    except Exception:
        pass
out = []
for i, (x, y, w, h) in enumerate(boxes):
    crops = [f[y:y + h, x:x + w] for f in frames]
    final = crops[-1]
    ground = float(np.median(final))
    ink = np.abs(final - ground) > 28
    if not ink.any():
        ink = np.ones_like(final, dtype=bool)
    span = float(np.mean(np.abs(final - ground)[ink])) or 1.0
    scores = [max(0.0, 1 - float(np.mean(np.abs(c - final)[ink])) / span) for c in crops]
    s = len(scores) - 1
    while s > 0 and scores[s - 1] >= 0.98:
        s -= 1
    glyphs = any(v < 0.9 for v in scores[:s])
    last = scores[s - 1] if s > 0 else 1.0
    out.append({'box': i, 'settle': s, 'last': round(last, 3), 'glyphs': glyphs, 'tail': [round(v, 2) for v in scores[max(0, s - 8):s + 1]]})
    k = 3
    seq = crops[max(0, s - 8):s + 1]
    im = Image.new('L', (len(seq) * (w * k + 4), h * k + 14), 0)
    dr = ImageDraw.Draw(im)
    for j, c in enumerate(seq):
        im.paste(Image.fromarray(c.astype(np.uint8)).resize((w * k, h * k), Image.NEAREST), (j * (w * k + 4), 14))
        dr.text((j * (w * k + 4) + 2, 1), f'{scores[max(0, s - 8) + j]:.2f}', fill=255)
    im.save(os.path.join(d, f'end-{i}.png'))
print(json.dumps(out))
