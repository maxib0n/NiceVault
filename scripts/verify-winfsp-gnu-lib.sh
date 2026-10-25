#!/usr/bin/env bash
# Checks the prebuilt MinGW delay-import library third_party/winfsp-sys/winfsp/lib-gnu/libwinfsp-x64.a
# against what it claims to be, with llvm-nm only (no MinGW binutils needed):
#   1. winfsp-x64.def lists exactly the exports of upstream's MSVC import lib (lib/winfsp-x64.lib);
#   2. the .a defines one thunk per .def export (+ the tail-merge stub) and nothing else as code;
#   3. its only external reference is MinGW's __delayLoadHelper2;
#   4. prints the SHA-256 recorded in lib-gnu/README.
# Regenerating it byte for byte needs binutils' x86_64-w64-mingw32-dlltool (see that README).
set -euo pipefail
D=$(cd "$(dirname "$0")/../third_party/winfsp-sys/winfsp/lib-gnu" && pwd)
NM=${NM:-llvm-nm}
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
"$NM" "$D/../lib/winfsp-x64.lib" | grep -o ' __imp_.*' | sed 's/ __imp_//' | sort -u | { echo 'LIBRARY winfsp-x64.dll'; echo EXPORTS; cat; } >"$T/def"
diff -u "$D/winfsp-x64.def" "$T/def" || { echo "FAIL: .def differs from the MSVC import lib's exports"; exit 1; }
tail -n +3 "$D/winfsp-x64.def" | sort >"$T/exports"
"$NM" "$D/libwinfsp-x64.a" | awk '$2=="T"{print $3}' | grep -v '^__tailMerge_libwinfsp_x64_a$' | sort >"$T/thunks"
diff -u "$T/exports" "$T/thunks" || { echo "FAIL: code symbols are not exactly one thunk per export"; exit 1; }
und=$("$NM" "$D/libwinfsp-x64.a" | awk '$1=="U"{print $2}' | sort -u | grep -v -x -e __delayLoadHelper2 -e __libwinfsp_x64_a_iname -e __tailMerge_libwinfsp_x64_a || true)
[ -z "$und" ] || { echo "FAIL: unexpected external references: $und"; exit 1; }
echo "ok: $(wc -l <"$T/exports") exports, thunks only, external: __delayLoadHelper2"
sha256sum "$D/libwinfsp-x64.a" "$D/winfsp-x64.def"
