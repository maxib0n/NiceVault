#!/usr/bin/env bash
# Real-app suite: NiceVault's Tauri shell with the real encrypted backend,
# driven through its real WebKitGTK webview on a private Xvfb display.
#
#   pnpm e2e:real            (from app/ui)   or   bash app/ui/e2e/real/e2e-real.sh
#
# Needs (no sudo):
#   tauri-driver       cargo install tauri-driver --locked
#   WebKitWebDriver    from Fedora's webkitgtk6.0 rpm (same WebKit as webkit2gtk4.1):
#                        dnf download webkitgtk6.0 && rpm2cpio webkitgtk6.0-*.rpm | cpio -idm ./usr/bin/WebKitWebDriver
#                      -> ~/.local/share/nv-e2e/bin/WebKitWebDriver (or set NV_E2E_WEBKITDRIVER)
#   xdotool + libxdo   dnf download xdotool libxdo, rpm2cpio
#                      -> ~/.local/share/nv-e2e/bin/xdotool, ~/.local/share/nv-e2e/lib/libxdo.so.3
#   Xvfb, ImageMagick `import`, python3-gobject (GTK 3), fusermount3 (for the mount step)
# Env: NV_E2E_OUT=<dir> keeps screenshots there (default app/ui/e2e/real/out),
#      NV_E2E_SCREEN=WxH sizes the X screen or the nested KWin's (3200x2000 for GDK_SCALE=2 runs),
#      NV_E2E_NO_BUILD=1 skips `pnpm build` + `cargo build`,
#      NV_E2E_SCRIPT=reveal.mjs runs another script of this folder instead of run.mjs
#      NV_E2E_DBUS=1 gives the app a private session bus (dbus-daemon, killed on exit) and the
#      single-instance guard on it (tray.mjs: the tray icon, closing to the tray, a second launch),
#      NV_E2E_DISPLAY=wayland runs the app in a headless nested KWin (kwin_wayland --virtual, no root)
#      instead of Xvfb: the GPU / DMA-BUF path of a real Wayland desktop (use it for jank.mjs; the
#      xdotool steps of run.mjs need X),
#      (reveal.mjs: proves the glyph decode plays in the real webview, with captured frames;
#      media-disk.mjs: no decrypted media in /var/tmp while a video plays — the
#      stream with `loop` and the Blob, checked through the web process's fds;
#      pm-perf-real.mjs: password-manager list timings with 2,000 entries in a real volume —
#      with NV_E2E_NO_BUILD=1 and CARGO_TARGET_DIR of another build it measures that build).
#
# Everything it starts (Xvfb, tauri-driver, the app, drag sources) is killed on
# exit, any FUSE mount under its private runtime dir is unmounted, and the
# scratch dir (volume, fixtures, export) is deleted.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
UI=$(cd "$HERE/../.." && pwd)
ROOT=$(cd "$UI/../.." && pwd)
TOOLS=${NV_E2E_TOOLS:-$HOME/.local/share/nv-e2e}
WKD=${NV_E2E_WEBKITDRIVER:-$TOOLS/bin/WebKitWebDriver}
XDO=${NV_E2E_XDOTOOL:-$(command -v xdotool || echo "$TOOLS/bin/xdotool")}
TD=${NV_E2E_TAURI_DRIVER:-$(command -v tauri-driver || echo "$HOME/.cargo/bin/tauri-driver")}
OUT=${NV_E2E_OUT:-$HERE/out}

for t in "$WKD" "$XDO" "$TD"; do [ -x "$t" ] || { echo "missing tool: $t (see header of $0)"; exit 2; }; done
command -v Xvfb >/dev/null && command -v import >/dev/null || { echo "need Xvfb and ImageMagick import"; exit 2; }

if [ "${NV_E2E_NO_BUILD:-0}" != 1 ]; then
  (cd "$UI" && pnpm build >/dev/null)
  (cd "$ROOT" && cargo build -p nicevault-app)
fi
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/nicevault"
[ -x "$BIN" ] || { echo "no app binary at $BIN"; exit 2; }

WORK=$(mktemp -d "${TMPDIR:-/tmp}/nv-e2e.XXXXXX")
mkdir -p "$WORK/stick" "$WORK/host" "$WORK/export" "$WORK/run" "$WORK/home" "$OUT"
chmod 700 "$WORK/run"
rm -f "$OUT"/*.png
cp "$BIN" "$WORK/stick/nicevault"   # Benvenuto scans the exe's folder: a fresh "stick"

# fixtures: real files of each previewable kind (the mock's sample assets)
M="$UI/public/mock"
cp "$M/photo-01.jpg" "$WORK/host/photo.jpg"
cp "$M/photo-02.jpg" "$WORK/host/photo 2.jpg"
cp "$M/document.pdf" "$WORK/host/document.pdf"
cp "$M/clip.webm" "$WORK/host/clip.webm"
cp "$M/readme.txt" "$WORK/host/readme.txt"
echo "trascinato da un file manager" >"$WORK/host/dropped one.txt"
echo "dentro la cartella" >"$WORK/host/into-folder.txt"
# password manager: an export with raw bookkeeping columns (AUTOFILLURLS / CREATETIME / MODIFYTIME)
cp "$ROOT/crates/nv-core/tests/fixtures/autofill.csv" "$WORK/host/export.csv"
# no site-icon fetches from the suite (they would go to the real sites): off in settings.json
mkdir -p "$WORK/home/.config/NiceVault"
echo '{ "site_icons": false }' >"$WORK/home/.config/NiceVault/settings.json"

PIDS=()
cleanup() {
  set +e
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
  pkill -f "$WORK/stick/nicevault" 2>/dev/null
  pkill -f "$HERE/drag-source.py" 2>/dev/null
  [ -n "${WL:-}" ] && pkill -f "kwin_wayland --virtual --socket $WL" 2>/dev/null
  sleep 0.5
  for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
  pkill -9 -f "$WORK/stick/nicevault" 2>/dev/null
  # FUSE mounts can only live under our private runtime dir
  grep -o " $WORK/run/[^ ]*" /proc/mounts 2>/dev/null | while read -r mp; do fusermount3 -u "$mp" || fusermount3 -uz "$mp"; done
  if grep -q " $WORK/" /proc/mounts; then echo "WARNING: still mounted under $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT INT TERM

SCREEN=${NV_E2E_SCREEN:-1600x1000}
if [ "${NV_E2E_DISPLAY:-x11}" = wayland ]; then
  # private nested Wayland compositor (GPU path, like a KDE session)
  WL="wl-nv-$$"
  # (its session bus address goes to $WORK/kwin-bus: capture-kwin.py takes screenshots through it,
  # allowed by a desktop entry for python in the private HOME — nothing else is on that bus)
  mkdir -p "$WORK/home/.local/share/applications"
  printf '[Desktop Entry]\nType=Application\nName=nv capture\nExec=%s\nNoDisplay=true\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n' \
    "$(readlink -f "$(command -v python3)")" >"$WORK/home/.local/share/applications/nv-capture.desktop"
  env -i HOME="$WORK/home" PATH=/usr/bin:/bin XDG_RUNTIME_DIR="$WORK/run" KWIN_COMPOSE=O2ES \
    dbus-run-session -- sh -c 'printf %s "$DBUS_SESSION_BUS_ADDRESS" >"$0/kwin-bus"; exec kwin_wayland "$@"' "$WORK" --virtual --socket "$WL" --width "${SCREEN%x*}" --height "${SCREEN#*x}" \
    --no-lockscreen --no-global-shortcuts --no-kactivities >"$WORK/kwin.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 100); do [ -e "$WORK/run/$WL" ] && break; sleep 0.1; done
  [ -e "$WORK/run/$WL" ] || { echo "kwin_wayland did not start"; tail -n 20 "$WORK/kwin.log"; exit 2; }
else
  # private X display
  DISP=99
  while [ -e "/tmp/.X11-unix/X$DISP" ] || [ -e "/tmp/.X$DISP-lock" ]; do DISP=$((DISP + 1)); done
  Xvfb ":$DISP" -screen 0 ${SCREEN}x24 -nolisten tcp >"$WORK/xvfb.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 50); do [ -e "/tmp/.X11-unix/X$DISP" ] && break; sleep 0.1; done
fi

# a throw-away HOME: GTK dialogs show no personal "Recent" files, no user settings leak in
# the suite reads Italian: the webview takes its language from the locale (lib/i18n.svelte.ts)
export LANG=it_IT.UTF-8 LC_ALL=it_IT.UTF-8 LANGUAGE=it
if [ -n "${WL:-}" ]; then
  export DISPLAY= GDK_BACKEND=wayland WAYLAND_DISPLAY="$WL"
else
  export DISPLAY=":$DISP" GDK_BACKEND=x11 WAYLAND_DISPLAY=
fi
export XDG_RUNTIME_DIR="$WORK/run" NO_AT_BRIDGE=1 HOME="$WORK/home"
# the drives the app sees: a file the suite writes (drives.rs NICEVAULT_FAKE_DRIVES), never the host's
export NICEVAULT_FAKE_DRIVES="$WORK/drives.json"
# the guard of single.rs: tests start throw-away copies next to the user's real session
# (NV_E2E_DBUS=1: a private bus of our own, so the guard can stay on there)
[ "${NV_E2E_DBUS:-0}" = 1 ] || export NICEVAULT_ALLOW_MULTI=1
# reveal.mjs: a slow drive (fs_list takes 400 ms), so the loading state hands over to the decode,
# and a slow scan (recent_list 600 ms), so Benvenuto is on screen before what it decodes with
[ "${NV_E2E_SCRIPT:-}" = reveal.mjs ] && export NICEVAULT_TEST_LIST_DELAY_MS=400 NICEVAULT_TEST_RECENTS_DELAY_MS=600
# run.mjs: its small clip goes through the loopback stream (normally only media over 32 MB do)
[ "${NV_E2E_SCRIPT:-run.mjs}" = run.mjs ] && export NICEVAULT_TEST_MEDIA_RAM_MAX=0
export LD_LIBRARY_PATH="$TOOLS/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
unset DBUS_SESSION_BUS_ADDRESS
if [ "${NV_E2E_DBUS:-0}" = 1 ]; then
  BUS=$(dbus-daemon --session --fork --nopidfile --print-address=1 --print-pid=1 --address="unix:path=$WORK/run/bus")
  PIDS+=($(printf '%s\n' "$BUS" | sed -n 2p))
  export DBUS_SESSION_BUS_ADDRESS=$(printf '%s\n' "$BUS" | sed -n 1p)
fi

PORT=4444
while (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; do PORT=$((PORT + 2)); done
"$TD" --port "$PORT" --native-port $((PORT + 1)) --native-driver "$WKD" >"$WORK/tauri-driver.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 50); do (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null && break; sleep 0.1; done

set +e
NV_E2E_APP="$WORK/stick/nicevault" NV_E2E_WORK="$WORK" NV_E2E_OUT="$OUT" NV_E2E_XDO="$XDO" \
  NV_E2E_DRIVER="http://127.0.0.1:$PORT" node "$HERE/${NV_E2E_SCRIPT:-run.mjs}"
rc=$?
set -e
[ $rc -ne 0 ] && { echo "--- tauri-driver log (tail)"; tail -n 30 "$WORK/tauri-driver.log"; }
exit $rc
