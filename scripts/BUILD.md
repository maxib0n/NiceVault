# Building NiceVault for Linux, Windows and macOS from Linux

`scripts/build-all.sh` builds the UI once (`pnpm -C app/ui build`), then:

| Output (in `dist/`, gitignored) | Target | How |
|---|---|---|
| `NiceVault-linux-x86_64`, `NiceVault-linux-x86_64.tar.gz` | `x86_64-unknown-linux-gnu` | `cargo build` in an Ubuntu 22.04 podman container, see "The Linux build" (the tar.gz holds the same binary) |
| `NiceVault-linux-x86_64.AppImage` | same | Tauri's AppImage bundler, see "The AppImage build" |
| `NiceVault-windows-x86_64.exe`, `NiceVault-windows-x86_64.zip` | `x86_64-pc-windows-gnu` | `cargo zigbuild`, zig as C compiler and linker (the zip holds `NiceVault.exe`) |
| `NiceVault-macos-universal.zip` | `aarch64-apple-darwin` + `x86_64-apple-darwin` | `cargo zigbuild` against the Xcode macOS SDK, `llvm-lipo`, hand-made `NiceVault.app`, ad-hoc signed with `rcodesign` |

`scripts/build-all.sh windows macos` builds a subset; `NV_SKIP_UI=1` reuses `app/ui/dist`.
Cargo output goes to `target-cross/` (its own target dir, so it never waits on the lock of
`target/`).

## Prerequisites (all user-level, no sudo)

- Rust targets: `rustup target add x86_64-pc-windows-gnu x86_64-apple-darwin aarch64-apple-darwin`
- `cargo install --locked cargo-zigbuild apple-codesign` (`apple-codesign` provides `rcodesign`)
- zig 0.16 (`/usr/bin/zig`), pnpm, python3, zip, `llvm-lipo`
- LLVM tools `llvm-objdump`, `llvm-ar`, `llvm-objcopy` and a **libclang of the same major version
  as zig's clang** (zig 0.16 = clang 21): the swiftly toolchain in
  `~/.local/share/swiftly/toolchains/*/usr` provides all of them. Fedora's libclang 22 does not
  work with zig's clang 21 builtin headers when bindgen runs for `winfsp-sys`.
- GNU binutils for MinGW, for `windres` (Windows icon/manifest resource) and `objdump`:
  ```
  mkdir -p ~/.local/share/mingw64-binutils/rpm && cd ~/.local/share/mingw64-binutils/rpm
  dnf download mingw64-binutils mingw-binutils-generic
  cd .. && for r in rpm/*.rpm; do rpm2cpio "$r" | cpio -idm; done
  ```
- macOS SDK: the one xtool extracted from Xcode, at
  `~/.swiftpm/swift-sdks/darwin.artifactbundle/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk`
- Perl modules for the vendored OpenSSL build in `~/.local/share/nv-perl` (as already used by
  `.cargo/config.toml`).

## Environment the script sets

| Variable | Value | Why |
|---|---|---|
| `CARGO_TARGET_DIR` | `target-cross/` (if unset) | separate from other builds |
| `SDKROOT` | the xtool MacOSX.sdk (if unset) | zig links against its frameworks (AppKit, WebKit, ...) |
| `MACOSX_DEPLOYMENT_TARGET` | `11.0` arm64, `10.15` x86_64 | `LSMinimumSystemVersion` is 10.15 |
| `CARGO_TARGET_<mac triple>_RUSTFLAGS` | `+ -C link-arg=-Wl,-headerpad_max_install_names` | room for the code signature |
| `LIBCLANG_PATH` | swiftly `usr/lib` (if unset) | bindgen for `winfsp-sys`, see above |
| `PERL5LIB` | `~/.local/share/nv-perl/...` (if unset) | OpenSSL `Configure` |
| `PATH` | + `scripts/cross/bin`, mingw binutils, swiftly `usr/bin` | windres, its preprocessor, LLVM tools |
| `CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER` | `scripts/cross/zig-mingw-link.sh` | see WebView2 below |
| `NV_CROSS_CACHE` | `target-cross/cross-cache` | static WebView2 loader archive, .app staging |

Override `NV_DIST_DIR`, `NV_MINGW_BINUTILS`, `SDKROOT` or `LIBCLANG_PATH` to relocate things.

## Windows details

- **ABI**: MinGW (`-gnu`), so no MSVC SDK or licence is needed. C runtime is the UCRT
  (`api-ms-win-crt-*.dll`, part of Windows 10/11).
- **WinFsp stays optional**: `winfsp-sys` only supports MSVC, so `third_party/winfsp-sys` is a
  patched copy (`[patch.crates-io]` in `Cargo.toml`). For `-gnu` it links **no** WinFsp
  library: its build.rs generates, from `winfsp/lib-gnu/winfsp-x64.def`, one jump thunk per
  export through a slot table, and `winfsp_sys::nv_bind_module` fills the slots with
  `GetProcAddress` after nv-mount has `LoadLibraryW`-ed `bin\winfsp-x64.dll` from the registry's
  `InstallDir`. Missing exports keep a stub returning STATUS_PROCEDURE_NOT_FOUND, and nv-mount
  refuses (reason `winfsp_unsupported`) if any it needs is missing.
  Until v0.2.0 this used `lib-gnu/libwinfsp-x64.a`, a `dlltool -y` delay-import library: once
  linked by zig's lld its descriptor's IAT/INT RVAs pointed into an empty piece of `.didat`
  (the head object's zero-size `.didat$4/$5` landed in a different output section from the
  entries), so `__delayLoadHelper2` read garbage and the first WinFsp call (pressing "Unità di
  sistema") crashed the app. The file stays for reference only; build-all.sh now fails if the
  exe has a `.didat` section. Bindgen gets `-fms-extensions` so `__declspec(align)` is
  honoured; the struct layouts match the upstream MSVC bindings.
- **Diagnostic log** (Windows): `%LOCALAPPDATA%\NiceVault\logs\nicevault.log`: WinFsp
  detection, every mount step, panics and native crashes (exception code, module+offset).
- **WebView2 loader is linked in**: on `-gnu`, `webview2-com-sys` links `WebView2Loader.dll`,
  which would have to ship next to the exe. `scripts/cross/zig-mingw-link.sh` swaps that for
  Microsoft's `WebView2LoaderStatic.lib` (from the same crate) plus
  `scripts/cross/webview2-static-shim.c`, which supplies the few MSVC runtime symbols it needs
  (operator new/delete, magic statics, /GS cookie, CFG dispatch). One file to ship.
- **Runtime requirement**: the Microsoft Edge WebView2 Runtime (preinstalled on Windows 11 and
  current Windows 10). WinFsp only for "mount as drive".
- The script fails if the exe imports any non-system DLL, is not GUI-subsystem, lacks the
  WinFsp loader, or still has a dlltool `.didat` delay-import section.

## macOS details

- `NiceVault.app/Contents/{Info.plist, PkgInfo, MacOS/nicevault, Resources/icon.icns}`;
  bundle id and version come from `app/src-tauri/tauri.conf.json`.
- **Ad-hoc signature** (`rcodesign sign`, no certificate): required for arm64 code to run at all.
  It is not a Developer ID signature and the app is not notarized, so:
  - downloaded from the internet (browser, AirDrop, mail: the file gets the
    `com.apple.quarantine` attribute), Gatekeeper refuses it ("cannot be opened" / "damaged").
    Remove the attribute: `xattr -dr com.apple.quarantine /Applications/NiceVault.app`, or on
    macOS 15+ try to open it once, then System Settings > Privacy & Security > "Open Anyway".
  - copied from a USB stick or another local disk there is usually no quarantine; if macOS
    still asks, right-click (Ctrl-click) the app > Apri / Open > Apri.
- Both slices are linked with `-headerpad_max_install_names`: zig's linker writes an ad-hoc
  signature only for arm64 and leaves no room in the x86_64 header for `rcodesign` to add one.
- The zip is made with `zip -r -y` (keeps the executable bit and symlinks); unzip it with
  Finder/Archive Utility or `ditto -x -k`.
- The script fails if the binary links anything outside `/usr/lib` and `/System/Library`.

## What can and cannot be verified on Linux

Verified by the script: import tables / linked dylibs, PE subsystem, Mach-O architectures and
signature. Under wine the Windows exe loads all imports, creates its window and gets as far as
the WebView2 loader, which reports that no WebView2 runtime is installed (expected under wine).
Not verifiable here: actually running the UI on Windows or macOS, WinFsp mounts, NFS mounts on
macOS, Gatekeeper behaviour.

## Updates and releases

The app updates itself in place (`app/src-tauri/src/updater.rs`; tauri-plugin-updater only
installs AppImage/NSIS/MSI/.app.tar.gz, not the bare files):

- **Where**: the one URL in `app/src-tauri/tauri.conf.json` `plugins.updater.endpoints[0]`, an
  asset of the latest release of the **public** repo
  (`https://github.com/<owner>/<repo>/releases/latest/download/latest.json`; rename the repo by
  changing that one line, `scripts/release.mjs` derives everything else from it). Plain HTTPS
  GETs: no token, no `Authorization` header, nothing secret in the app, nothing encrypted.
  Artefact URLs must be on the manifest's host (GitHub's redirect to its asset CDN is followed
  and must stay HTTPS).
- **Trust**: every artefact is signed with the minisign key
  `~/.config/nicevault/updater-secret.key` (empty password, never committed); its public half is
  `plugins.updater.pubkey`. The signature's trusted comment binds the version and the
  platforms; a bad or missing signature, an old version or another platform means nothing is
  written.
- **Assets** of a release (the website links to the first four by these exact names):

  | File | What |
  |---|---|
  | `NiceVault-linux-x86_64.AppImage` | Linux, self-contained (WebKitGTK inside); also what USB sticks carry |
  | `NiceVault-linux-x86_64.tar.gz` | Linux, the bare binary (needs the system's WebKitGTK 4.1) |
  | `NiceVault-windows-x86_64.zip` | Windows, `NiceVault.exe` |
  | `NiceVault-macos-universal.zip` | macOS, `NiceVault.app` (arm64 + x86_64, ad-hoc signed) |
  | `NiceVault-linux-x86_64`, `NiceVault-windows-x86_64.exe` | the bare files the updater and the stick download |
  | `<file>.sig`, `SHA256SUMS`, `latest.json` | signatures, checksums, what the app reads |

  `latest.json`: `{version, notes (en), notes_i18n {it, en}, pub_date, platforms}` with
  `linux-x86_64-appimage` (AppImage), `linux-x86_64` (bare binary), `windows-x86_64` (exe),
  `darwin-aarch64` and `darwin-x86_64` (both the universal zip), each `{signature, url}`.
- **Install**: the new file is written next to the running one (`<name>.new`, fsync), the
  current one is kept as `<name>.old`, the new one is renamed in (Linux: hard link + atomic
  rename; Windows: the running exe is renamed away first; macOS: the whole `X.app` is swapped,
  name kept, files byte-identical to the signed zip so the ad-hoc signature stays valid).
  Running from an AppImage (`$APPIMAGE`), the file replaced is the `.AppImage` itself, from the
  `linux-x86_64-appimage` asset; the bare binary never replaces an AppImage. Rollback by hand:
  delete the new file and rename `.old` back. Not writable (installed system-wide) or a dev
  build: the verified file goes to Downloads and the user is told.
- **When**: at start at most once a day (off switch on Benvenuto), and "Cerca aggiornamenti".
  Installed: `updater.json` in the OS config dir; portable: in memory, so from a stick the
  check runs at every start and the off switch lasts until the app quits.
- **USB sticks**: the Linux program on the NiceVault partition is the AppImage (it runs on a
  distro with no WebKitGTK; it needs FUSE 2 or `--appimage-extract-and-run`), under the stick's
  usual name `NiceVault-linux-x86_64`. The programs are plain files on the stick, so it keeps
  working offline.
- Test against a local server: `NICEVAULT_UPDATE_URL=http://127.0.0.1:8000/latest.json`.

### The Linux build (glibc 2.35 container)

A binary linked on the build host needs that host's glibc (Fedora 44: 2.43) and fails on older
distros ("GLIBC_2.43 not found"). So `scripts/build-all.sh` builds the Linux artefacts inside a
rootless podman container made from `scripts/linux-build.Containerfile` (Ubuntu 22.04, glibc
2.35, WebKitGTK 4.1; Rust, Node and pnpm pinned there): it runs itself again in there with
`NV_LINUX_HOST=1 NV_SKIP_UI=1 ... linux`, the repo, `dist/` and the target dir mounted at their
own paths. The image is tagged `localhost/nicevault-linux-build:<hash of the Containerfile>` and
built on first use (and after every Containerfile change). After the build the script checks
that the newest `GLIBC_` symbol version the binary needs is at most 2.35.

| Variable | Default | |
|---|---|---|
| `NV_LINUX_TARGET_DIR` | `$CARGO_TARGET_DIR-glibc` (`target-cross-glibc`) | the container's target dir; never the host's (its build scripts are linked against the container's glibc) |
| `NV_LINUX_CACHE` | `.cache/linux-build` | `cargo-registry`, `cargo-git` and `xdg` (Tauri's linuxdeploy downloads), kept between builds |
| `NV_LINUX_HOST=1` | unset | build the Linux artefacts on this host instead (local testing only: they need its glibc) |

### The AppImage build

`scripts/build-all.sh linux` builds the bare binary, the tar.gz and the AppImage. The AppImage
comes from Tauri's bundler (`tauri build --bundles appimage`, run from `app/ui/node_modules`'
CLI with a config override: the committed `tauri.conf.json` keeps `bundle.active` false, the
other targets are built by hand). First use downloads `linuxdeploy`, its GTK plugin and the
AppImage plugin into `$XDG_CACHE_HOME/tauri` (default `~/.cache/tauri`; no root needed; copy
that folder to build offline). Needs the system WebKitGTK/GTK development packages the normal
Linux build already needs, plus `patchelf`-free tooling only. Two environment variables are set
by the script: `NO_STRIP=1` (linuxdeploy's bundled `strip` cannot read the `.relr.dyn` sections
of current distros' libraries) and `APPIMAGE_EXTRACT_AND_RUN=1` (the downloaded tools run
without FUSE). Running an AppImage needs FUSE 2 (`libfuse.so.2`) or `--appimage-extract-and-run`.

### Cutting a release

`scripts/release.mjs`, like Latente's; pure parts tested by `scripts/release.test.mjs`, run by
`pnpm test`:

```
node scripts/release.mjs --dry-run          # next version from `Change:` trailers, changelog preview
node scripts/release.mjs                    # bump Cargo.toml / tauri.conf.json / package.json,
                                            # CHANGELOG.md, commit, tag vX.Y.Z, notes draft
                                            # (--pre beta|rc, --version X.Y.Z to force)
$EDITOR release-notes/vX.Y.Z.json           # user notes, it + en; delete _README
node scripts/release.mjs publish --dry-run  # build-all, sign, dist/release/ (assets, .sig,
                                            # SHA256SUMS, latest.json); prints the upload
node scripts/release.mjs publish            # gh release create vX.Y.Z ... on the public repo
```

`publish` uploads with your own `gh auth login`; no token is read, embedded or printed.
`--skip-build` reuses what is in `dist/` (`NV_DIST_DIR` overrides the folder).

**One-time bridge for 0.2.x.** Apps up to 0.2.x read the old private repo with an embedded
token and expect encrypted `.nvpack` assets there. After the first public release,
`node scripts/release.mjs bridge [--dry-run]` repacks the same signed bare builds in that old
layout (key: `scripts/legacy/release-pack.key`, the secret those apps carry) and creates the
same version's release on the old repo, so they update to the first build that reads the public
repo. Nothing more is ever published there. Remove `bridge`, `scripts/legacy/` and `LEGACY_REPO`
once no 0.2.x install is left.

`node scripts/set-version.mjs [X.Y.Z]` shows or sets the version by hand. An app only offers
an update newer than itself, so test with an older build.

## Android (debug APK)

`scripts/build-android.sh [--target x86_64]` → `dist/NiceVault-android-debug.apk` (arm64-v8a + x86_64,
signed with the debug keystore `~/.android/debug.keystore`). User-level prerequisites: Android SDK at
`~/Android/Sdk` (platform 36, build-tools 36, NDK 29: `sdkmanager "platforms;android-36" "ndk;29.0.14206865"`),
JDK 21, `rustup target add aarch64-linux-android x86_64-linux-android`. The script builds the UI, runs
`tauri android build --debug --apk` from `app/src-tauri` (the generated Gradle project is committed in
`app/src-tauri/gen/android`), strips the Rust debug info (`CARGO_PROFILE_DEV_STRIP=debuginfo`) and packages
clean (AGP otherwise keeps the old APK's space). Release signing: you create a keystore and a
`gen/android/keystore.properties` (never committed).

Emulator (tested on Fedora, kernel 7.2): emulator 36.6.11 segfaults with KVM; 37.3.5 runs under Xvfb
with `-gpu host`. To keep your main SDK untouched it can live in a separate SDK root (`$NV_ANDROID_SDK`) with the non-Play
`system-images;android-35;google_apis;x86_64` (adb is authorised without a prompt there):
```
S=$NV_ANDROID_SDK   # a scratch SDK root: licenses copied, platform-tools linked from ~/Android/Sdk
sdkmanager --sdk_root=$S --channel=3 emulator "system-images;android-35;google_apis;x86_64"
ANDROID_SDK_ROOT=$S xvfb-run -a -s "-screen 0 1280x1024x24" $S/emulator/emulator -avd nv_mobile -port 5584 \
  -no-audio -no-boot-anim -no-snapshot -gpu host
adb -s emulator-5584 install -r dist/NiceVault-android-debug.apk
adb -s emulator-5584 shell am start -n app.nicevault.desktop/.MainActivity --ez nv_allow_screenshots true
```
The WebView is debuggable in debug builds: `adb forward tcp:9335 localabstract:webview_devtools_remote_<pid>`
then Chrome DevTools Protocol on `127.0.0.1:9335` (Runtime.evaluate, Input.dispatchTouchEvent).

## iOS (signed .ipa with xtool, no Mac)

`scripts/build-ios.sh [--debug] [--unsigned]` → `dist/NiceVault-ios.ipa`. How it works and the prerequisites
are in the script's header: Rust static library for `aarch64-apple-ios` (swiftly clang against xtool's
iPhoneOS SDK, `app/ios/toolchain/bin/{ios-clang,xcrun}`), Tauri's and the plugins' Swift packages linked by
SwiftPM (`app/ios/Package.swift`), `xtool dev build --sign --ipa` with the account xtool is logged in to.
**Use a personal Apple ID (free team).** A free-team build gets the
bundle id `XTL-<team>.app.nicevault.desktop` and a 7-day development profile. Install (iPhone on USB,
unlocked, trusted, Developer Mode on): `xtool devices`, then `xtool install dist/NiceVault-ios.ipa`; trust the
developer profile in Settings > General > VPN & Device Management. Rebuild / re-sign every 7 days.
