#!/usr/bin/env bash
# Debug APK of NiceVault for Android (arm64 + x86_64), all user-level. See scripts/BUILD.md "Android".
#   scripts/build-android.sh                 # debug APK, both ABIs
#   scripts/build-android.sh --target x86_64 # one ABI (emulator)
# Output: app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
#         (copied to dist/NiceVault-android-debug.apk)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
if [ -z "${NDK_HOME:-}" ]; then
  NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
  export NDK_HOME
fi
if [ -z "${JAVA_HOME:-}" ]; then
  JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v javac)")")")"
  export JAVA_HOME
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target-android}"
# AGP can't strip the Rust library without an ndkVersion pin: strip it in cargo (debug info
# made it ~380 MB per ABI; the debug build stays debuggable with panics' file:line)
export CARGO_PROFILE_DEV_STRIP="${CARGO_PROFILE_DEV_STRIP:-debuginfo}"
targets=("$@")
[ ${#targets[@]} -gt 0 ] || targets=(--target aarch64 --target x86_64)
if [ "${NV_SKIP_UI:-}" != 1 ]; then pnpm -C "$root/app/ui" build; fi
cd "$root/app/src-tauri"
# AGP packages incrementally into the old APK, keeping the space of replaced entries: start clean
rm -rf gen/android/app/build/outputs/apk gen/android/app/build/intermediates/apk
# beforeBuildCommand of tauri.conf.json is written for the desktop scripts; the UI is built above
"$root/app/ui/node_modules/.bin/tauri" android build --debug --apk "${targets[@]}" \
  --config '{"build":{"beforeBuildCommand":""}}'
apk="$root/app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
mkdir -p "$root/dist"
cp "$apk" "$root/dist/NiceVault-android-debug.apk"
echo "APK: $root/dist/NiceVault-android-debug.apk"
