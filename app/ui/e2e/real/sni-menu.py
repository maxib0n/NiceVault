#!/usr/bin/env python3
"""Clicks an item of the registered tray icon's menu, the way a panel does (com.canonical.dbusmenu
Event "clicked"), on the suite's private session bus.

  sni-menu.py <watcher log> <label>        -> prints the labels it saw; exit 1 if <label> is missing
"""
import sys
import time

import gi

gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib  # noqa: E402

log, label = sys.argv[1], sys.argv[2]
sender, path = open(log).read().split('\n')[0].split(' ', 1)
if not path.startswith('/'):
    sender, path = path, '/StatusNotifierItem'
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)


def call(dest, obj, iface, method, args, ret):
    return bus.call_sync(dest, obj, iface, method, args, GLib.VariantType(ret), Gio.DBusCallFlags.NONE, 3000, None).unpack()


(menu,) = call(sender, path, 'org.freedesktop.DBus.Properties', 'Get', GLib.Variant('(ss)', ('org.kde.StatusNotifierItem', 'Menu')), '(v)')
_, layout = call(sender, menu, 'com.canonical.dbusmenu', 'GetLayout', GLib.Variant('(iias)', (0, -1, [])), '(u(ia{sv}av))')
seen, target = [], None


def walk(node):
    global target
    i, props, children = node
    text = props.get('label', '').replace('_', '')
    if text:
        seen.append(text)
        if text == label:
            target = i
    for c in children:
        walk(c)


walk(layout)
print(' | '.join(seen))
if target is None:
    sys.exit(1)
bus.call_sync(sender, menu, 'com.canonical.dbusmenu', 'Event', GLib.Variant('(isvu)', (target, 'clicked', GLib.Variant('i', 0), int(time.time()))), None, Gio.DBusCallFlags.NONE, 3000, None)
