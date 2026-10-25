fn main() {
    // WinFsp is delay-loaded so the exe starts on machines without it (nv-mount checks for
    // the DLL at runtime). Cargo does not forward nv-mount's link args to the final binary,
    // so they are repeated here (same lines as crates/nv-mount/build.rs).
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dll = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default().as_str() {
            "x86_64" => "winfsp-x64.dll",
            "aarch64" => "winfsp-a64.dll",
            _ => "winfsp-x86.dll",
        };
        println!("cargo::rustc-link-lib=dylib=delayimp");
        println!("cargo::rustc-link-arg=/DELAYLOAD:{dll}");
    }
    tauri_build::build()
}

