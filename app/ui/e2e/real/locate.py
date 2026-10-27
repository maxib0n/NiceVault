#!/usr/bin/env python3
"""Where the webview sits on the captured screen (Wayland has no window positions to ask for):
the offset at which the WebDriver screenshot of the page best matches a captured frame.

  locate.py <screen frame.png> <webdriver screenshot.png>   -> {"x": .., "y": ..}
"""
import json
import sys

import numpy as np
from PIL import Image

scr = np.asarray(Image.open(sys.argv[1]).convert('L'), dtype=np.float32)
pg = Image.open(sys.argv[2]).convert('L')
sk = int(sys.argv[3]) if len(sys.argv) > 3 else 1
pg = np.asarray(pg.resize((pg.width // sk, pg.height // sk)), dtype=np.float32)
k = 2
s = scr[::k, ::k]
p = pg[::k, ::k]
ph, pw = p.shape
best = (1e18, 0, 0)
for y in range(0, s.shape[0] - ph + 1):
    for x in range(0, s.shape[1] - pw + 1):
        d = np.mean(np.abs(s[y : y + ph, x : x + pw] - p))
        if d < best[0]:
            best = (d, x, y)
_, bx, by = best
best = (1e18, bx * k, by * k)
for y in range(max(0, by * k - k), min(scr.shape[0] - pg.shape[0], by * k + k) + 1):
    for x in range(max(0, bx * k - k), min(scr.shape[1] - pg.shape[1], bx * k + k) + 1):
        d = np.mean(np.abs(scr[y : y + pg.shape[0] : 2, x : x + pg.shape[1] : 2] - pg[::2, ::2]))
        if d < best[0]:
            best = (d, x, y)
print(json.dumps({'x': int(best[1]) * sk, 'y': int(best[2]) * sk, 'err': round(float(best[0]), 2)}))
