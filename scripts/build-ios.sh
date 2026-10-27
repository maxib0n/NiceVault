#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
ios="$root/app/ios"

profile=release
sign=1
for a in "$@"; do
  case "$a" in
    --debug) profile=debug ;;
    --unsigned) sign=0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

die() { echo "build-ios: $*" >&2; exit 1; }

xtool="${XTOOL:-$(command -v xtool || echo "$HOME/.local/bin/xtool")}"
[ -x "$xtool" ] || die "xtool not found (https://github.com/xtool-org/xtool, then 'xtool setup')"
sdkbundle="${NV_DARWIN_SDK:-$HOME/.swiftpm/swift-sdks/darwin.artifactbundle}"
export SDKROOT="${SDKROOT:-$sdkbundle/Developer/Platforms/iPhoneOS.platform/Developer/SDKs/iPhoneOS.sdk}"
[ -d "$SDKROOT/System/Library/Frameworks/UIKit.framework" ] || die "no iPhoneOS SDK at $SDKROOT ('xtool setup' installs it)"

if [ -z "${NV_LLVM:-}" ]; then
  NV_LLVM="$(ls -d "$HOME"/.local/share/swiftly/toolchains/*/usr 2>/dev/null | sort -V | tail -1)"
fi
[ -x "$NV_LLVM/bin/clang" ] || die "no clang in NV_LLVM=$NV_LLVM"
export NV_IOS_CLANG="$NV_LLVM/bin/clang"

min=15.0   # keep in sync with app/ios/Package.swift platforms
target=aarch64-apple-ios
export IPHONEOS_DEPLOYMENT_TARGET="$min"
export NV_IOS_TRIPLE="arm64-apple-ios$min"
tc="$ios/toolchain/bin"
export PATH="$tc:$NV_LLVM/bin:$PATH"
export CC_aarch64_apple_ios="$tc/ios-clang" CXX_aarch64_apple_ios="$tc/ios-clang"
export AR_aarch64_apple_ios="$NV_LLVM/bin/llvm-ar" RANLIB_aarch64_apple_ios="$NV_LLVM/bin/llvm-ranlib"
export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$tc/ios-clang"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target-ios}"
export PERL5LIB="${PERL5LIB:-$HOME/.local/share/nv-perl/usr/share/perl5:$HOME/.local/share/nv-perl/usr/share/perl5/vendor_perl:$HOME/.local/share/nv-perl/usr/lib64/perl5:$HOME/.local/share/nv-perl/usr/lib64/perl5/vendor_perl}"

if [ "${NV_SKIP_UI:-}" != 1 ]; then pnpm -C "$root/app/ui" build; fi
[ -f "$root/app/ui/dist/index.html" ] || die "app/ui/dist is missing (pnpm -C app/ui build)"

cargo_profile=()
[ "$profile" = release ] && cargo_profile=(--release)
(cd "$root" && cargo rustc -p nicevault-app --lib --crate-type staticlib --target "$target" "${cargo_profile[@]}")
lib="$CARGO_TARGET_DIR/$target/$profile/libnicevault_app.a"
[ -f "$lib" ] || die "cargo did not produce $lib"
mkdir -p "$ios/Libs"
ln -sfn "$lib" "$ios/Libs/libnicevault_app.a"

meta="$(cd "$root" && cargo metadata --format-version 1 --filter-platform "$target")"
pkgdir() {
  python3 -I -c '
import json, os, sys
m = json.load(sys.stdin)
ids = {n["id"] for n in m["resolve"]["nodes"]}
for p in m["packages"]:
    if p["name"] == sys.argv[1] and p["id"] in ids:
        print(os.path.dirname(p["manifest_path"])); break
' "$1" <<<"$meta"
}
extra="$(python3 -I -c '
import json, os, sys
m = json.load(sys.stdin)
ids = {n["id"] for n in m["resolve"]["nodes"]}
known = {"tauri", "tauri-plugin-dialog", "tauri-plugin-clipboard-manager"}
for p in m["packages"]:
    d = os.path.dirname(p["manifest_path"])
    if p["id"] in ids and p["name"] not in known and os.path.exists(os.path.join(d, "ios", "Package.swift")):
        print(p["name"])
' <<<"$meta")"
[ -z "$extra" ] || die "crates with an iOS Swift package not in app/ios/Package.swift: $extra"
stage() { # <src dir> <dest under app/ios/Packages>
  [ -f "$1/Package.swift" ] || die "no Package.swift in $1"
  mkdir -p "$ios/Packages/$2"
  rsync -a --delete "$1/" "$ios/Packages/$2/"
}
stage "$(pkgdir tauri)/mobile/ios-api" .tauri/tauri-api   # the plugins refer to ../.tauri/tauri-api
stage "$(pkgdir tauri-plugin-dialog)/ios" tauri-plugin-dialog
stage "$(pkgdir tauri-plugin-clipboard-manager)/ios" tauri-plugin-clipboard-manager

xargs=(-c "$profile")
[ $sign = 1 ] && xargs+=(--sign --ipa)
(cd "$ios" && "$xtool" dev build "${xargs[@]}" </dev/null)

if [ $sign = 1 ]; then
  out="$ios/xtool/NiceVault.ipa"
  [ -f "$out" ] || die "xtool did not write $out"
  tmp="$(mktemp -d)"
  (cd "$tmp" && unzip -q "$out" && chmod 755 Payload/NiceVault.app/NiceVault \
    && rm -f "$out" && zip -yqr0 -X "$out" Payload)
  rm -rf "$tmp"
  mkdir -p "$root/dist"
  cp "$out" "$root/dist/NiceVault-ios.ipa"
  echo "IPA: $root/dist/NiceVault-ios.ipa"
else
  echo "App: $ios/xtool/NiceVault.app (unsigned)"
fi
