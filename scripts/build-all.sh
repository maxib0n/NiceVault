#!/usr/bin/env bash
# Build NiceVault for Linux x86_64, Windows x86_64 and macOS (universal) from a Linux box.
#
#   scripts/build-all.sh                 # everything
#   scripts/build-all.sh windows macos   # a subset (linux | windows | macos)
#   NV_SKIP_UI=1 scripts/build-all.sh    # reuse the existing app/ui/dist
#
# Outputs (dist/, gitignored):
#   dist/NiceVault-linux-x86_64.AppImage  self-contained (WebKitGTK inside); the stick carries this one
#   dist/NiceVault-linux-x86_64.tar.gz    the bare binary (needs the system's WebKitGTK 4.1)
#   dist/NiceVault-linux-x86_64           the same binary, what the bare-binary build self-updates from
#   dist/NiceVault-windows-x86_64.exe     single exe, WebView2 loader linked in, WinFsp loaded at runtime
#   dist/NiceVault-windows-x86_64.zip     the exe, zipped, for the website
#   dist/NiceVault-macos-universal.zip    NiceVault.app (arm64 + x86_64), ad-hoc signed
#   dist/SHA256SUMS
# Prerequisites and the environment this sets: scripts/BUILD.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CROSS="$ROOT/scripts/cross"
DIST="${NV_DIST_DIR:-$ROOT/dist}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target-cross}"

# ---- toolchain locations (override any of these from the environment) -------------------
SWIFTLY_TC="$(command ls -d "$HOME"/.local/share/swiftly/toolchains/*/usr 2>/dev/null | sort -V | tail -1 || true)"
export SDKROOT="${SDKROOT:-$HOME/.swiftpm/swift-sdks/darwin.artifactbundle/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk}"
# bindgen (winfsp-sys) runs libclang with zig's clang builtin headers: use a libclang of the
# same major version as `zig cc` (zig 0.16 = clang 21; Fedora's system libclang is newer).
export LIBCLANG_PATH="${LIBCLANG_PATH:-${SWIFTLY_TC:+$SWIFTLY_TC/lib}}"
MINGW_BINUTILS="${NV_MINGW_BINUTILS:-$HOME/.local/share/mingw64-binutils/usr/bin}"
# Perl modules missing from the base system, needed by the vendored OpenSSL's Configure.
NV_PERL="$HOME/.local/share/nv-perl"
if [[ -z "${PERL5LIB:-}" && -d "$NV_PERL" ]]; then
    export PERL5LIB="$NV_PERL/usr/share/perl5:$NV_PERL/usr/share/perl5/vendor_perl:$NV_PERL/usr/lib64/perl5:$NV_PERL/usr/lib64/perl5/vendor_perl"
fi
export PATH="$CROSS/bin:$MINGW_BINUTILS:${SWIFTLY_TC:+$SWIFTLY_TC/bin:}$PATH"
export NV_CROSS_CACHE="$CARGO_TARGET_DIR/cross-cache"

MAC_MIN_X86=10.15   # Tauri 2 / WKWebView floor for Intel Macs
MAC_MIN_ARM=11.0    # first macOS on Apple silicon

PKG=nicevault-app
BIN=nicevault

log()  { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
die()  { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing tool: $1 ($2)"; }
size() { du -h "$1" | cut -f1; }

targets=("$@")
[[ ${#targets[@]} -eq 0 ]] && targets=(linux windows macos)
want() { local t; for t in "${targets[@]}"; do [[ "$t" == "$1" ]] && return 0; done; return 1; }

# ---- preflight ---------------------------------------------------------------------------
need cargo "rustup"
need pnpm "volta/corepack"
if want windows || want macos; then
    need zig "/usr/bin/zig"
    need cargo-zigbuild "cargo install --locked cargo-zigbuild"
    need llvm-objdump "swiftly toolchain or llvm"
fi
if want windows; then
    rustup target list --installed | grep -x x86_64-pc-windows-gnu >/dev/null || die "rustup target add x86_64-pc-windows-gnu"
    need x86_64-w64-mingw32-windres "mingw64-binutils extracted into $MINGW_BINUTILS (see BUILD.md)"
    need x86_64-w64-mingw32-objdump "mingw64-binutils"
    need llvm-ar "swiftly toolchain or llvm"
    need llvm-objcopy "swiftly toolchain or llvm"
    [[ -n "$LIBCLANG_PATH" && -d "$LIBCLANG_PATH" ]] || die "LIBCLANG_PATH: need a libclang matching zig's clang major version"
fi
if want macos; then
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        rustup target list --installed | grep -x "$t" >/dev/null || die "rustup target add $t"
    done
    [[ -d "$SDKROOT/System/Library/Frameworks/WebKit.framework" ]] || die "SDKROOT is not a macOS SDK: $SDKROOT"
    need llvm-lipo "llvm"
    need rcodesign "cargo install --locked apple-codesign"
    need zip "zip"
fi
mkdir -p "$DIST" "$NV_CROSS_CACHE"

# ---- UI (once, shared by every target) ---------------------------------------------------
if [[ -z "${NV_SKIP_UI:-}" ]]; then
    log "UI: pnpm build"
    [[ -d "$ROOT/app/ui/node_modules" ]] || pnpm -C "$ROOT/app/ui" install --frozen-lockfile
    pnpm -C "$ROOT/app/ui" build
fi
[[ -f "$ROOT/app/ui/dist/index.html" ]] || die "app/ui/dist/index.html missing"

cd "$ROOT"

# ---- Linux x86_64 (native) -----------------------------------------------------------------
if want linux; then
    log "Linux x86_64"
    cargo build --release -p "$PKG" --bin "$BIN"
    cp "$CARGO_TARGET_DIR/release/$BIN" "$DIST/NiceVault-linux-x86_64"
    echo "dist/NiceVault-linux-x86_64 ($(size "$DIST/NiceVault-linux-x86_64"))"
    # tar.gz: one folder holding the binary
    TGZ_STAGE="$NV_CROSS_CACHE/linux-tgz"
    rm -rf "$TGZ_STAGE" && mkdir -p "$TGZ_STAGE/NiceVault-linux-x86_64"
    cp "$CARGO_TARGET_DIR/release/$BIN" "$TGZ_STAGE/NiceVault-linux-x86_64/nicevault"
    tar -C "$TGZ_STAGE" --owner=0 --group=0 --numeric-owner -czf "$DIST/NiceVault-linux-x86_64.tar.gz" NiceVault-linux-x86_64
    echo "dist/NiceVault-linux-x86_64.tar.gz ($(size "$DIST/NiceVault-linux-x86_64.tar.gz"))"

    # AppImage: Tauri's bundler (linuxdeploy + the GTK plugin, downloaded into $XDG_CACHE_HOME/tauri
    # on first use, no root needed). NO_STRIP: linuxdeploy's bundled strip chokes on the .relr.dyn
    # sections of current distros' libraries; APPIMAGE_EXTRACT_AND_RUN: no FUSE needed to build.
    log "Linux x86_64 AppImage"
    rm -rf "$CARGO_TARGET_DIR/release/bundle/appimage"
    (cd "$ROOT/app/src-tauri" && NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1 ../ui/node_modules/.bin/tauri build --bundles appimage \
        --config '{"build":{"beforeBuildCommand":""},"bundle":{"active":true,"targets":["appimage"]}}')
    shopt -s nullglob
    images=("$CARGO_TARGET_DIR"/release/bundle/appimage/*.AppImage)
    shopt -u nullglob
    [[ ${#images[@]} -eq 1 ]] || die "expected one AppImage in $CARGO_TARGET_DIR/release/bundle/appimage"
    cp "${images[0]}" "$DIST/NiceVault-linux-x86_64.AppImage"
    chmod 755 "$DIST/NiceVault-linux-x86_64.AppImage"
    echo "dist/NiceVault-linux-x86_64.AppImage ($(size "$DIST/NiceVault-linux-x86_64.AppImage"))"
fi

# ---- Windows x86_64 (MinGW ABI, zig as C compiler and linker) ----------------------------
if want windows; then
    log "Windows x86_64 (x86_64-pc-windows-gnu)"
    T=x86_64-pc-windows-gnu
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$CROSS/zig-mingw-link.sh" \
        cargo zigbuild --release --target "$T" -p "$PKG" --bin "$BIN"
    EXE="$CARGO_TARGET_DIR/$T/release/$BIN.exe"

    log "Windows: import table check"
    pe="$(x86_64-w64-mingw32-objdump -p "$EXE")"
    imports="$(sed -n 's/^\s*DLL Name: //p' <<<"$pe" | sort -fu)"
    echo "$imports" | sed 's/^/    /'
    # Only DLLs that ship with Windows 10/11. WinFsp is bound at runtime (LoadLibraryW + GetProcAddress,
    # winfsp-sys gnu_thunks), never imported; WebView2Loader static.
    system_dll='^((api-ms-win-[a-z0-9-]+|advapi32|bcrypt|bcryptprimitives|combase|comctl32|comdlg32|crypt32|d3d11|dxgi|msimg32|ncrypt|powrprof|wtsapi32|mpr|dbghelp|dwmapi|gdi32|imm32|kernel32|ntdll|ole32|oleaut32|pdh|propsys|secur32|shcore|shell32|shlwapi|user32|userenv|uxtheme|version|winmm|ws2_32|credui|iphlpapi|netapi32|psapi|rpcrt4|setupapi|winhttp|wininet)\.dll|winspool\.drv)$'
    bad="$(echo "$imports" | grep -viE "$system_dll" || true)"
    [[ -z "$bad" ]] || die "non-system DLL imports: $bad"
    grep -q 'Subsystem.*Windows GUI' <<<"$pe" || die "exe is not a GUI-subsystem binary"
    grep -qa 'winfsp-x64.dll' "$EXE" || die "WinFsp runtime loader (DLL name) not found"
    ! x86_64-w64-mingw32-objdump -h "$EXE" | grep -q '\.didat' || die "exe still has a dlltool delay-import section (.didat): it crashes on the first WinFsp call"
    cp "$EXE" "$DIST/NiceVault-windows-x86_64.exe"
    echo "dist/NiceVault-windows-x86_64.exe ($(size "$DIST/NiceVault-windows-x86_64.exe"))"
    need zip "zip"
    rm -f "$DIST/NiceVault-windows-x86_64.zip"
    WIN_STAGE="$NV_CROSS_CACHE/windows-zip"
    rm -rf "$WIN_STAGE" && mkdir -p "$WIN_STAGE"
    cp "$EXE" "$WIN_STAGE/NiceVault.exe"
    (cd "$WIN_STAGE" && zip -q -X "$DIST/NiceVault-windows-x86_64.zip" NiceVault.exe)
    echo "dist/NiceVault-windows-x86_64.zip ($(size "$DIST/NiceVault-windows-x86_64.zip"))"
fi

# ---- macOS universal .app -------------------------------------------------------------------
if want macos; then
    for T in aarch64-apple-darwin x86_64-apple-darwin; do
        log "macOS $T"
        if [[ "$T" == aarch64-* ]]; then min=$MAC_MIN_ARM; else min=$MAC_MIN_X86; fi
        # zig's linker only reserves room for (and writes) an ad-hoc signature on arm64;
        # headerpad leaves space for rcodesign to add LC_CODE_SIGNATURE to the x86_64 slice.
        tvar="CARGO_TARGET_$(tr 'a-z-' 'A-Z_' <<<"$T")_RUSTFLAGS"
        env "$tvar=${!tvar:-} -C link-arg=-Wl,-headerpad_max_install_names" \
            MACOSX_DEPLOYMENT_TARGET="$min" cargo zigbuild --release --target "$T" -p "$PKG" --bin "$BIN"
    done

    log "macOS: universal binary + NiceVault.app"
    read -r IDENT VERSION < <(python3 -c 'import json,sys;c=json.load(open(sys.argv[1]));print(c["identifier"],c["version"])' app/src-tauri/tauri.conf.json)
    STAGE="$NV_CROSS_CACHE/macos"
    APP="$STAGE/NiceVault.app"
    rm -rf "$STAGE" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
    llvm-lipo -create -output "$APP/Contents/MacOS/$BIN" \
        "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/$BIN" \
        "$CARGO_TARGET_DIR/x86_64-apple-darwin/release/$BIN"
    chmod 755 "$APP/Contents/MacOS/$BIN"
    cp app/src-tauri/icons/icon.icns "$APP/Contents/Resources/icon.icns"
    printf 'APPL????' > "$APP/Contents/PkgInfo"
    cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key><string>en</string>
	<key>CFBundleDisplayName</key><string>NiceVault</string>
	<key>CFBundleName</key><string>NiceVault</string>
	<key>CFBundleExecutable</key><string>$BIN</string>
	<key>CFBundleIdentifier</key><string>$IDENT</string>
	<key>CFBundleIconFile</key><string>icon.icns</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>$VERSION</string>
	<key>CFBundleVersion</key><string>$VERSION</string>
	<key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
	<key>LSMinimumSystemVersion</key><string>$MAC_MIN_X86</string>
	<key>LSArchitecturePriority</key><array><string>arm64</string><string>x86_64</string></array>
	<key>NSHighResolutionCapable</key><true/>
	<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
	<key>NSRequiresAquaSystemAppearance</key><false/>
</dict>
</plist>
PLIST

    log "macOS: linked dylibs check"
    dylibs="$(llvm-objdump --macho --dylibs-used --arch=all "$APP/Contents/MacOS/$BIN" | grep -E '^\s+/' | awk '{print $1}' | sort -u)"
    echo "$dylibs" | sed 's/^/    /'
    bad="$(echo "$dylibs" | grep -vE '^/(usr/lib|System/Library)/' || true)"
    [[ -z "$bad" ]] || die "non-system dylibs linked: $bad"

    log "macOS: ad-hoc signature (rcodesign)"
    rcodesign sign "$APP" >/dev/null 2>&1 || die "rcodesign sign failed"
    # (`rcodesign verify` cannot check ad-hoc signatures: it expects a CMS blob.)
    sig="$(rcodesign print-signature-info "$APP/Contents/MacOS/$BIN")"
    [[ "$(grep -c 'flags: CodeSignatureFlags(ADHOC)' <<<"$sig")" -ge 2 ]] || die "both slices must be ad-hoc signed"
    grep -q "identifier: $IDENT" <<<"$sig" || die "signature identifier is not $IDENT"
    [[ -f "$APP/Contents/_CodeSignature/CodeResources" ]] || die "bundle CodeResources missing"
    echo "    ad-hoc signed, identifier $IDENT, both slices"

    rm -f "$DIST/NiceVault-macos-universal.zip"
    (cd "$STAGE" && zip -q -r -y -X "$DIST/NiceVault-macos-universal.zip" NiceVault.app)
    file "$APP/Contents/MacOS/$BIN"
    echo "dist/NiceVault-macos-universal.zip ($(size "$DIST/NiceVault-macos-universal.zip"))"
fi

(cd "$DIST" && sha256sum NiceVault-* > SHA256SUMS 2>/dev/null || true)
log "done: $DIST"
command ls -l "$DIST"
