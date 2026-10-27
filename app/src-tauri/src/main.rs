#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    nicevault_app::hardening::no_dumps();
    let args: Vec<String> = std::env::args().collect();
    if nv_bridge::is_host_invocation(&args) {
        std::process::exit(nv_bridge::host::run(&args));
    }
    nicevault_app::diaglog::init();
    if let Some(i) = args.iter().position(|a| a == "--device-helper") {
        let code = match args.get(i + 1) {
            Some(p) => nicevault_app::device::helper::run(std::path::Path::new(p), args.get(i + 2).map(String::as_str)),
            None => 2,
        };
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--device-dry-run") {
        std::process::exit(nicevault_app::device::cmd::dry_run_cli(&args));
    }
    nicevault_app::run();
}
