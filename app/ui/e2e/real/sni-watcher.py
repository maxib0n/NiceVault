#!/usr/bin/env python3
"""A stand-in for the desktop's tray (the KDE / AppIndicator StatusNotifierWatcher) on the
suite's private session bus: owns org.kde.StatusNotifierWatcher, says a host is there, and logs
each icon that registers ("<sender> <path or service>" per line) to <log>. Runs until SIGTERM.

  sni-watcher.py <log>       (DBUS_SESSION_BUS_ADDRESS: the private bus)
"""
import signal
import sys

import warnings

import gi

gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib  # noqa: E402

warnings.simplefilter('ignore', DeprecationWarning)

LOG = sys.argv[1]
XML = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem"><arg name="service" type="s" direction="in"/></method>
    <method name="RegisterStatusNotifierHost"><arg name="service" type="s" direction="in"/></method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>"""
items = []


def call(conn, sender, path, iface, method, params, inv):
    if method == 'RegisterStatusNotifierItem':
        (svc,) = params.unpack()
        item = f'{sender} {svc}'
        items.append(item)
        with open(LOG, 'a') as f:
            f.write(item + '\n')
        conn.emit_signal(None, '/StatusNotifierWatcher', 'org.kde.StatusNotifierWatcher', 'StatusNotifierItemRegistered', GLib.Variant('(s)', (item,)))
    inv.return_value(None)


def prop(conn, sender, path, iface, name):
    return {
        'RegisteredStatusNotifierItems': GLib.Variant('as', items),
        'IsStatusNotifierHostRegistered': GLib.Variant('b', True),
        'ProtocolVersion': GLib.Variant('i', 0),
    }[name]


def acquired(conn, name):
    info = Gio.DBusNodeInfo.new_for_xml(XML).interfaces[0]
    conn.register_object('/StatusNotifierWatcher', info, call, prop, None)


loop = GLib.MainLoop()
Gio.bus_own_name(Gio.BusType.SESSION, 'org.kde.StatusNotifierWatcher', Gio.BusNameOwnerFlags.NONE, acquired, None, lambda *_: loop.quit())
signal.signal(signal.SIGTERM, lambda *_: loop.quit())
loop.run()
