//! On Windows, link WinFsp as a delay-loaded DLL so this crate's own test/example binaries
//! start without WinFsp installed. Cargo does not forward these flags to dependents: the
//! final executable's build.rs must emit the same two lines (or call
//! `winfsp::build::winfsp_link_delayload()`).
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "msvc" {
        return;
    }
    let dll = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default().as_str() {
        "x86_64" => "winfsp-x64.dll",
        "aarch64" => "winfsp-a64.dll",
        _ => "winfsp-x86.dll",
    };
    println!("cargo::rustc-link-lib=dylib=delayimp");
    println!("cargo::rustc-link-arg=/DELAYLOAD:{dll}");
}
