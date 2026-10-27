import Tauri
import tauri_plugin_dialog
import tauri_plugin_clipboard_manager

@_silgen_name("start_app")
func start_app()

@main
enum NiceVaultMain {
    static func main() {
        start_app()
    }
}
