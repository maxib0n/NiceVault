#!/usr/bin/env bash
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

M="$UI/public/mock"
cp "$M/photo-01.jpg" "$WORK/host/photo.jpg"
cp "$M/photo-02.jpg" "$WORK/host/photo 2.jpg"
cp "$M/document.pdf" "$WORK/host/document.pdf"
cp "$M/clip.webm" "$WORK/host/clip.webm"
cp "$M/readme.txt" "$WORK/host/readme.txt"
echo "trascinato da un file manager" >"$WORK/host/dropped one.txt"
echo "dentro la cartella" >"$WORK/host/into-folder.txt"
cp "$ROOT/crates/nv-core/tests/fixtures/autofill.csv" "$WORK/host/export.csv"
mkdir -p "$WORK/home/.config/NiceVault"
echo '{ "site_icons": false }' >"$WORK/home/.config/NiceVault/settings.json"

PIDS=()
cleanup() {
  set +e
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
  pkill -s 0 -f "$WORK/stick/nicevault" 2>/dev/null
  pkill -s 0 -f "$HERE/drag-source.py" 2>/dev/null
  [ -n "${WL:-}" ] && pkill -s 0 -f "kwin_wayland --virtual --socket $WL" 2>/dev/null
  sleep 0.5
  for p in "${PIDS[@]}"; do kill -9 "$p" 2>/dev/null; done
  pkill -9 -s 0 -f "$WORK/stick/nicevault" 2>/dev/null
  grep -o " $WORK/run/[^ ]*" /proc/mounts 2>/dev/null | while read -r mp; do fusermount3 -u "$mp" || fusermount3 -uz "$mp"; done
  if grep -q " $WORK/" /proc/mounts; then echo "WARNING: still mounted under $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT INT TERM

SCREEN=${NV_E2E_SCREEN:-1600x1000}
if [ "${NV_E2E_DISPLAY:-x11}" = wayland ]; then
  WL="wl-nv-$$"
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
  DISP=99
  while [ -e "/tmp/.X11-unix/X$DISP" ] || [ -e "/tmp/.X$DISP-lock" ]; do DISP=$((DISP + 1)); done
  Xvfb ":$DISP" -screen 0 ${SCREEN}x24 -nolisten tcp >"$WORK/xvfb.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 50); do [ -e "/tmp/.X11-unix/X$DISP" ] && break; sleep 0.1; done
fi

export LANG=it_IT.UTF-8 LC_ALL=it_IT.UTF-8 LANGUAGE=it
if [ -n "${WL:-}" ]; then
  export DISPLAY= GDK_BACKEND=wayland WAYLAND_DISPLAY="$WL"
else
  export DISPLAY=":$DISP" GDK_BACKEND=x11 WAYLAND_DISPLAY=
fi
export PLAYWRIGHT_BROWSERS_PATH="${PLAYWRIGHT_BROWSERS_PATH:-$HOME/.cache/ms-playwright}"
export XDG_RUNTIME_DIR="$WORK/run" NO_AT_BRIDGE=1 HOME="$WORK/home"
export NICEVAULT_FAKE_DRIVES="$WORK/drives.json"
[ "${NV_E2E_DBUS:-0}" = 1 ] || export NICEVAULT_ALLOW_MULTI=1
[ "${NV_E2E_SCRIPT:-}" = reveal.mjs ] && export NICEVAULT_TEST_LIST_DELAY_MS=400 NICEVAULT_TEST_RECENTS_DELAY_MS=600
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
