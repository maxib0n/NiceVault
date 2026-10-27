#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cache="${NV_CROSS_CACHE:?NV_CROSS_CACHE must point to a scratch dir}/webview2-static"
mkdir -p "$cache"

wv_dir=""
prev=""
for a in "$@"; do
    if [[ "$prev" == "-L" && -f "$a/WebView2LoaderStatic.lib" ]]; then wv_dir="$a"; fi
    prev="$a"
done

args=()
if [[ -n "$wv_dir" ]]; then
    lib="$cache/libwebview2loader_static.a"
    shim_src="$here/webview2-static-shim.c"
    if [[ ! -f "$lib" || "$lib" -ot "$wv_dir/WebView2LoaderStatic.lib" || "$lib" -ot "$shim_src" ]]; then
        rm -rf "$cache/obj" && mkdir -p "$cache/obj"
        (cd "$cache/obj" && llvm-ar x "$wv_dir/WebView2LoaderStatic.lib")
        while IFS= read -r -d '' o; do
            if llvm-objcopy --dump-section ".drectve=$cache/drectve" "$o" 2>/dev/null; then
                sed -E 's#/DEFAULTLIB:[^ ]+ ?##g' "$cache/drectve" > "$cache/drectve.new"
                llvm-objcopy --update-section ".drectve=$cache/drectve.new" "$o"
            fi
        done < <(find "$cache/obj" -type f -name '*.obj' -print0)
        zig cc -target x86_64-windows-gnu -O2 -fno-emulated-tls -c "$shim_src" -o "$cache/obj/nv_shim.o"
        rm -f "$lib.tmp"
        llvm-ar rcs "$lib.tmp" $(find "$cache/obj" -type f \( -name '*.obj' -o -name '*.o' \) | sort)
        mv "$lib.tmp" "$lib"
    fi
    for a in "$@"; do
        if [[ "$a" == "-lWebView2Loader.dll" ]]; then args+=("$lib"); else args+=("$a"); fi
    done
else
    args=("$@")
fi

exec cargo-zigbuild zig cc -- -g -fno-sanitize=all -target x86_64-windows-gnu "${args[@]}"
