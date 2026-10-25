#!/usr/bin/env bash
# Linker for x86_64-pc-windows-gnu (set as CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER by
# scripts/build-all.sh): zig cc, the same command cargo-zigbuild's own wrapper runs, except
# that `-lWebView2Loader.dll` (webview2-com-sys's import of WebView2Loader.dll on non-MSVC
# targets) is replaced by Microsoft's WebView2LoaderStatic.lib plus a small shim providing
# the MSVC runtime bits it needs. Result: a single .exe, no WebView2Loader.dll to ship.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cache="${NV_CROSS_CACHE:?NV_CROSS_CACHE must point to a scratch dir}/webview2-static"
mkdir -p "$cache"

# Find webview2-com-sys's out/x64 dir among the -L arguments.
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
        # Drop the objects' /DEFAULTLIB:uuid.lib,ole32.lib directives: lld would look for
        # libuuid.a/libole32.a on disk (zig generates its import libs on demand). The
        # symbols they provide come from the import libs rustc already links.
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
