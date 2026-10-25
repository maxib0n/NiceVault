// NiceVault desktop shell. See lib.rs.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    no_dumps();
    // Windows: %LOCALAPPDATA%\NiceVault\logs\nicevault.log (mount steps, panics, crashes)
    nicevault_app::diaglog::init();
    // Before Tauri: the Windows elevated helper of "Crea dispositivo" (no window), and the dry
    // run that prints the candidates, a plan and its commands without executing anything.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--device-helper") {
        let code = match args.get(i + 1) {
            Some(p) => nicevault_app::device::helper::run(std::path::Path::new(p)),
            None => 2,
        };
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--device-dry-run") {
        std::process::exit(nicevault_app::device::cmd::dry_run_cli(&args));
    }
    nicevault_app::run();
}

/// A crash must not write the process image (master keys, decrypted pages, passwords) to the
/// host's disk, and another process of the same user must not read our memory.
/// Release builds abort on panic, so any panic would otherwise leave a core dump behind.
fn no_dumps() {
    #[cfg(unix)]
    unsafe {
        // SAFETY: plain syscalls on this process, no pointers kept.
        let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
        // Also blocks ptrace and /proc/<pid>/mem from same-user processes (root still can).
        #[cfg(target_os = "linux")]
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetErrorMode(mode: u32) -> u32;
        }
        #[link(name = "wer")]
        unsafe extern "system" {
            fn WerSetFlags(flags: u32) -> i32;
        }
        // SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX; WER_FAULT_REPORTING_FLAG_NOHEAP | _NO_UI:
        // an error report never carries the heap (where the keys live). Nothing is written to the registry.
        unsafe {
            SetErrorMode(0x0001 | 0x0002);
            WerSetFlags(0x0001 | 0x0020);
        }
    }
}
