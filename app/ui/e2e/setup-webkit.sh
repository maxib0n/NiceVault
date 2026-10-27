#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

npx playwright install chromium webkit || true   # host validation fails on Fedora; browsers are still downloaded

WK=$(ls -d "$HOME"/.cache/ms-playwright/webkit-* | sort -V | tail -1)
missing=$(for f in "$WK"/minibrowser-wpe/bin/* "$WK"/minibrowser-wpe/lib/*.so*; do
  LD_LIBRARY_PATH="$WK/minibrowser-wpe/lib:$WK/minibrowser-wpe/sys/lib" ldd "$f" 2>/dev/null | grep 'not found' || true
done | sort -u)
if [ -z "$missing" ]; then echo "WebKit: all libraries present"; exit 0; fi
echo "WebKit is missing:"; echo "$missing"

command -v dnf >/dev/null || { echo "not a dnf system: install the libraries above with your package manager"; exit 1; }
LIBS="${NV_E2E_LIBS:-$HOME/.local/share/nv-e2e/pwlib}"
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
mkdir -p "$LIBS"

if echo "$missing" | grep -q 'libicu.*\.so\.74'; then
  (cd "$TMP" && dnf download --releasever=40 --arch x86_64 libicu >/dev/null && mkdir icu && cd icu && rpm2cpio ../libicu-74*.rpm | cpio -idm 2>/dev/null)
  cp -a "$TMP"/icu/usr/lib64/libicu{uc,i18n,data}.so.74* "$LIBS"/
fi
if echo "$missing" | grep -q 'libjpeg\.so\.8'; then
  (cd "$TMP" && dnf download --source libjpeg-turbo >/dev/null && mkdir jpeg && cd jpeg && rpm2cpio ../libjpeg-turbo-*.src.rpm | cpio -idm 2>/dev/null \
    && tar xf libjpeg-turbo-*.tar.gz && cd libjpeg-turbo-*/ && mkdir b && cd b \
    && cmake .. -DWITH_JPEG8=1 -DWITH_SIMD=0 -DENABLE_STATIC=0 -DCMAKE_BUILD_TYPE=Release >/dev/null && make -j"$(nproc)" jpeg >/dev/null)
  cp -a "$TMP"/jpeg/libjpeg-turbo-*/b/libjpeg.so.8* "$LIBS"/
fi
for d in "$WK"/minibrowser-wpe/sys/lib "$WK"/minibrowser-gtk/sys/lib; do
  [ -d "$d" ] && cp -a "$LIBS"/* "$d"/
done
echo "WebKit libraries installed into $WK/minibrowser-*/sys/lib"
