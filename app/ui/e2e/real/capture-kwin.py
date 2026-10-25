#!/usr/bin/env python3
"""capture.py for the nested KWin (NV_E2E_DISPLAY=wayland): the composited screen, as the
compositor drew it, through org.kde.KWin.ScreenShot2 on the private KWin's session bus, as fast
as it answers, until SIGTERM. Same output: <dir>/f000.png... (greyscale, half size: <dir>/scale.txt) and <dir>/times.json.

  capture-kwin.py <dir> <bus address file>
"""
import json
import os
import signal
import sys
import time

import gi

gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib  # noqa: E402
import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

out, busfile = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
addr = open(busfile).read().strip()
bus = Gio.DBusConnection.new_for_address_sync(addr, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
stop = False
SCALE = 2
with open(os.path.join(out, 'scale.txt'), 'w') as fh:
    fh.write(str(SCALE))


def term(*_):
    global stop
    stop = True


signal.signal(signal.SIGTERM, term)
signal.signal(signal.SIGINT, term)


def grab():
    r, w = os.pipe()
    fds = Gio.UnixFDList.new()
    idx = fds.append(w)
    os.close(w)
    opts = {'native-resolution': GLib.Variant('b', True)}
    res = bus.call_with_unix_fd_list_sync(
        'org.kde.KWin', '/org/kde/KWin/ScreenShot2', 'org.kde.KWin.ScreenShot2', 'CaptureWorkspace',
        GLib.Variant('(a{sv}h)', (opts, idx)), GLib.VariantType('(a{sv})'), Gio.DBusCallFlags.NONE, 5000, fds, None)
    meta = res[0].unpack()[0]
    fds = None
    wdt, hgt, stride = meta['width'], meta['height'], meta['stride']
    need = stride * hgt
    buf = bytearray()
    with os.fdopen(r, 'rb') as f:
        while len(buf) < need:
            chunk = f.read(need - len(buf))
            if not chunk:
                break
            buf += chunk
    a = np.frombuffer(bytes(buf[:need]), dtype=np.uint8).reshape(hgt, stride)
    # QImage::Format_ARGB32 / RGB32 in memory: B G R A; the green channel of every second pixel
    # (a luma stand-in, cheap enough to keep up with the compositor; <dir>/scale says so)
    return a[::SCALE, 1 : wdt * 4 : 4 * SCALE].copy()


frames, times, t0 = [], [], time.monotonic()
while not stop and len(frames) < 600:
    try:
        frames.append(grab())
    except Exception as e:  # noqa: BLE001
        import traceback; traceback.print_exc()
        time.sleep(0.05)
        continue
    times.append(round((time.monotonic() - t0) * 1000))
    time.sleep(0.006)
for i, f in enumerate(frames):
    Image.fromarray(f).save(os.path.join(out, f'f{i:03d}.png'), compress_level=1)
with open(os.path.join(out, 'times.json'), 'w') as fh:
    json.dump(times, fh)
