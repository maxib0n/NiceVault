#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    no_dumps();
    nicevault_app::diaglog::init();
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

fn no_dumps() {
    #[cfg(unix)]
    unsafe {
        let zero = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        libc::setrlimit(libc::RLIMIT_CORE, &zero);
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
        unsafe {
            SetErrorMode(0x0001 | 0x0002);
            WerSetFlags(0x0001 | 0x0020);
        }
    }
}
