use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(feature = "system")]
use windows_registry::LOCAL_MACHINE;

static HEADER: &str = r#"
#include <winfsp/winfsp.h>
#include <winfsp/fsctl.h>
#include <winfsp/launch.h>
"#;

#[cfg(not(feature = "system"))]
fn local() -> String {
    let project_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // NiceVault patch: MinGW targets link a GNU *delay-import* library generated from the
    // MSVC import lib (see winfsp/lib-gnu/README), so the exe has no hard import on
    // winfsp-x64.dll. It must not sit next to winfsp-x64.lib: zig prefers `.lib` files.
    // NiceVault patch (2): MinGW no longer links any WinFsp library. The dlltool delay-import
    // lib produced a broken descriptor once linked by lld (its IAT/INT RVAs pointed into an
    // empty `.didat` piece, so the first WinFsp call crashed). Instead `gnu_thunks.rs`
    // (generated below from winfsp/lib-gnu/winfsp-x64.def) defines every export as a jump
    // through a slot that `nv_bind_module` fills with GetProcAddress at runtime.
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("gnu") {
        println!(
            "cargo:rustc-link-search={}",
            project_dir.join("winfsp/lib").to_string_lossy()
        );
    }

    "--include-directory=winfsp/inc".into()
}

#[cfg(feature = "system")]
fn system() -> String {
    if !cfg!(windows) {
        panic!("'system' feature not supported for cross-platform compilation.");
    }

    let directory = LOCAL_MACHINE
        .open("SOFTWARE\\WOW6432Node\\WinFsp")
        .ok()
        .and_then(|u| u.get_string("InstallDir").ok())
        .expect("WinFsp installation directory not found.");

    println!("cargo:rustc-link-search={}/lib", directory);

    format!("--include-directory={}/inc", directory)
}

fn copy_winfsp_dll(winfsp_lib: &str) {
    println!("cargo:rerun-if-env-changed=WINFSP_DLL_OUTPUT_PATH");

    // Get the output path from environment variable
    let dll_out_path = match env::var("WINFSP_DLL_OUTPUT_PATH") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            return;
        }
    };

    if let Err(e) = fs::create_dir_all(&dll_out_path) {
        panic!(
            "Failed to create WinFSP DLL output directory {}: {}",
            dll_out_path.display(),
            e
        );
    }

    let project_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let dll_path = project_dir
        .join("winfsp/bin")
        .join(format!("{}.dll", winfsp_lib));
    if !dll_path.exists() {
        panic!(
            "WinFSP DLL source file does not exist: {}",
            dll_path.display()
        );
    }

    let dll_dest = dll_out_path.join(format!("{}.dll", winfsp_lib));
    if let Err(e) = fs::copy(&dll_path, &dll_dest) {
        panic!(
            "Failed to copy {} to {}: {}",
            dll_path.display(),
            dll_dest.display(),
            e
        );
    }
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // host needs to be windows
    if cfg!(feature = "docsrs") {
        println!("cargo:warning=WinFSP does not build on any operating system but Windows. This feature is meant for docs.rs only. It will not link when compiled into a binary.");
        File::create(out_dir.join("bindings.rs")).unwrap();
        return;
    }

    // Use the target OS configuration instead of the host OS configuration to enable cross-platform compilation
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".to_string());
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "unknown".to_string());
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_else(|_| "unknown".to_string());

    if target_os != "windows" {
        panic!("WinFSP is only supported on Windows.");
    }

    #[cfg(feature = "system")]
    let link_include = system();
    #[cfg(not(feature = "system"))]
    let link_include = local();

    // NiceVault patch: also accept the MinGW target (`x86_64-pc-windows-gnu`), for
    // cross-compiling from Linux with zig. Delay-loading comes from the delay-import
    // library in winfsp/lib-gnu (its thunks call MinGW's `__delayLoadHelper2`); MSVC's
    // `delayimp` lib and `/DELAYLOAD` do not exist there.
    if target_env == "msvc" {
        println!("cargo:rustc-link-lib=dylib=delayimp");
    }

    // Architecture-specific configuration
    let (winfsp_lib, clang_target) = match (target_arch.as_str(), target_env.as_str()) {
        ("x86_64", "msvc") => ("winfsp-x64", "x86_64-pc-windows-msvc"),
        ("x86", "msvc") => ("winfsp-x86", "x86-pc-windows-msvc"),
        ("aarch64", "msvc") => ("winfsp-a64", "aarch64-pc-windows-msvc"),
        ("x86_64", "gnu") => ("winfsp-x64", "x86_64-pc-windows-gnu"),
        _ => panic!("unsupported triple {}", env::var("TARGET").unwrap()),
    };

    if target_env == "gnu" {
        write_gnu_thunks(&out_dir);
    } else {
        println!("cargo:rustc-link-lib=dylib={}", winfsp_lib);
    }
    if target_env == "msvc" {
        println!("cargo:rustc-link-arg=/DELAYLOAD:{}.dll", winfsp_lib);
    }

    let bindings_path_str = out_dir.join("bindings.rs");

    if !Path::new(&bindings_path_str).exists() {
        let gen_h_path = out_dir.join("gen.h");
        let mut gen_h = File::create(&gen_h_path).expect("could not create file");
        gen_h
            .write_all(HEADER.as_bytes())
            .expect("could not write header file");

        let bindings = bindgen::Builder::default()
            .header(gen_h_path.to_str().unwrap())
            .derive_default(true)
            .blocklist_type("_?P?IMAGE_TLS_DIRECTORY.*")
            .allowlist_function("Fsp.*")
            .allowlist_type("FSP.*")
            .allowlist_type("Fsp.*")
            .allowlist_var("FSP_.*")
            .allowlist_var("Fsp.*")
            .allowlist_var("CTL_CODE")
            .clang_arg("-DUNICODE")
            .clang_arg(link_include);

        let bindings = bindings.clang_arg(&format!("--target={}", clang_target));
        // NiceVault patch: the WinFsp headers are written for MSVC. With MinGW headers we need
        // `__declspec(align)` (-fms-extensions; without it struct layouts would be wrong),
        // C11 `static_assert`, and to keep MinGW's `_ReadWriteBarrier` macro from clashing
        // with fsctl.h's own declaration of it.
        let bindings = if target_env == "gnu" {
            bindings
                .clang_arg("-fms-extensions")
                .clang_arg("-Dstatic_assert=_Static_assert")
                .clang_arg("-D_ReadWriteBarrier=_ReadWriteBarrier")
        } else {
            bindings
        };


        let bindings = bindings
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("Unable to generate bindings");

        bindings
            .write_to_file(out_dir.join("bindings.rs"))
            .expect("Couldn't write bindings!");
    }

    #[cfg(not(feature = "system"))]
    copy_winfsp_dll(winfsp_lib);
}

/// NiceVault patch: runtime binding for MinGW (see `local()`). One slot per export, all
/// initialised to a stub returning STATUS_PROCEDURE_NOT_FOUND, so a missing export is an
/// NTSTATUS error rather than a jump to address 0.
fn write_gnu_thunks(out_dir: &Path) {
    let def = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("winfsp/lib-gnu/winfsp-x64.def");
    println!("cargo:rerun-if-changed={}", def.display());
    let text = fs::read_to_string(&def).expect("winfsp-x64.def");
    let names: Vec<&str> = text
        .lines()
        .skip_while(|l| !l.trim().eq_ignore_ascii_case("EXPORTS"))
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut asm = String::from(".data\n.p2align 3\n.globl __nvwfsp_slots\n__nvwfsp_slots:\n");
    for _ in &names {
        asm.push_str(".quad __nvwfsp_missing\n");
    }
    asm.push_str(".text\n.p2align 4\n__nvwfsp_missing:\nmovl $0xC000007A, %eax\nret\n");
    for (i, n) in names.iter().enumerate() {
        asm.push_str(&format!(".p2align 3\n.globl {n}\n{n}:\njmpq *(__nvwfsp_slots+{})(%rip)\n", i * 8));
    }
    let mut out = format!("pub(crate) const NAMES: [&str; {}] = [\n", names.len());
    for n in &names {
        out.push_str(&format!("    \"{n}\\0\",\n"));
    }
    out.push_str("];\n");
    out.push_str(&format!("core::arch::global_asm!({asm:?}, options(att_syntax));\n"));
    fs::write(out_dir.join("gnu_thunks.rs"), out).expect("gnu_thunks.rs");
}
