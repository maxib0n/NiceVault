#!/usr/bin/env python3
# A tiny X11 drag source for the real-app suite: a GTK3 window at (X, Y) that
# offers the given files as text/uri-list, exactly like a file manager does.
# Drive it with xdotool (press on it, move onto the target window, release).
#   drag-source.py X Y FILE...
# Prints "ready" once mapped and "drag-end" when a drag finishes.
import sys
from urllib.parse import quote

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk  # noqa: E402

x, y, files = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3:]
uris = ["file://" + quote(f) for f in files]

win = Gtk.Window(title="nv-drag-source")
win.set_decorated(False)
win.set_default_size(160, 120)
win.move(x, y)
box = Gtk.EventBox()
box.add(Gtk.Label(label="DRAG\n%d file" % len(files)))
win.add(box)

box.drag_source_set(Gdk.ModifierType.BUTTON1_MASK, [], Gdk.DragAction.COPY)
box.drag_source_add_uri_targets()


def on_get(_w, _ctx, data, _info, _time):
    data.set_uris(uris)


def on_end(_w, _ctx):
    print("drag-end", flush=True)


box.connect("drag-data-get", on_get)
box.connect("drag-end", on_end)
win.connect("destroy", Gtk.main_quit)
win.show_all()
GLib.timeout_add(300, lambda: print("ready", flush=True) and False)
Gtk.main()
