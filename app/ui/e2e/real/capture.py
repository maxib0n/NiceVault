#!/usr/bin/env python3
"""Captures the X root window as fast as it can (~every 15-25 ms) until SIGTERM, then writes
the frames as <dir>/f000.png... (greyscale) and <dir>/times.json (ms since the start).

  capture.py <dir>          (DISPLAY set by the caller; GTK 3 through python3-gobject)
"""
import json
import os
import signal
import sys
import time

import gi

gi.require_version('Gdk', '3.0')
from gi.repository import Gdk  # noqa: E402
import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
root = Gdk.get_default_root_window()
w, h = root.get_width(), root.get_height()
stop = False


def term(*_):
    global stop
    stop = True


signal.signal(signal.SIGTERM, term)
signal.signal(signal.SIGINT, term)
frames, times, t0 = [], [], time.monotonic()
while not stop and len(frames) < 600:
    pb = Gdk.pixbuf_get_from_window(root, 0, 0, w, h)
    if pb is None:
        continue
    a = np.frombuffer(pb.get_pixels(), dtype=np.uint8).reshape(h, pb.get_rowstride())[:, : w * pb.get_n_channels()]
    a = a.reshape(h, w, pb.get_n_channels())[:, :, :3]
    # luma, kept in memory until the end (encoding PNGs here would slow the capture down)
    frames.append((a[:, :, 0] * 0.299 + a[:, :, 1] * 0.587 + a[:, :, 2] * 0.114).astype(np.uint8))
    times.append(round((time.monotonic() - t0) * 1000))
    time.sleep(0.004)
for i, f in enumerate(frames):
    Image.fromarray(f).save(os.path.join(out, f'f{i:03d}.png'), compress_level=1)
with open(os.path.join(out, 'times.json'), 'w') as fh:
    json.dump(times, fh)
