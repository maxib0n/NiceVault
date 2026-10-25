// The whole app is Rust (app/src-tauri, `#[tauri::mobile_entry_point] fn run()`): the macro
// exports `start_app`, which sets up logging and calls tao's UIApplicationMain, like the
// `ffi::start_app()` in the main.mm that `tauri ios init` generates.
//
// The Swift packages below are imported so their @_cdecl entry points (init_plugin_dialog,
// init_plugin_clipboard, Tauri's log_stdout / on_webview_created / ...) are linked in.
import Tauri
import tauri_plugin_dialog
import tauri_plugin_clipboard_manager

@_silgen_name("start_app")
func start_app()

// A library target (xtool links it into its own executable), so `@main` instead of main.swift.
@main
enum NiceVaultMain {
    static func main() {
        start_app()
    }
}
