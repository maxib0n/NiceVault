#!/usr/bin/env bash
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
export CARGO_PROFILE_DEV_STRIP="${CARGO_PROFILE_DEV_STRIP:-debuginfo}"
targets=("$@")
[ ${#targets[@]} -gt 0 ] || targets=(--target aarch64 --target x86_64)
if [ "${NV_SKIP_UI:-}" != 1 ]; then pnpm -C "$root/app/ui" build; fi
cd "$root/app/src-tauri"
rm -rf gen/android/app/build/outputs/apk gen/android/app/build/intermediates/apk
"$root/app/ui/node_modules/.bin/tauri" android build --debug --apk "${targets[@]}" \
  --config '{"build":{"beforeBuildCommand":""}}'
apk="$root/app/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk"
mkdir -p "$root/dist"
cp "$apk" "$root/dist/NiceVault-android-debug.apk"
echo "APK: $root/dist/NiceVault-android-debug.apk"
