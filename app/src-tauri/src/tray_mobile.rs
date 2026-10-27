use tauri::{AppHandle, Manager};

#[derive(Default)]
pub struct Tray;

impl Tray {
    pub fn new(_launched_minimized: bool) -> Tray {
        Tray
    }

    pub fn active(&self) -> bool {
        false
    }

    pub fn unforce(&self) {}
}

pub fn supported() -> bool {
    false
}

pub fn host_available() -> bool {
    false
}

pub fn hides_on_close(_app: &AppHandle) -> bool {
    false
}

pub fn show_unless_hosted(_app: &AppHandle) {}

pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_focus();
    }
}

pub fn sync(_app: &AppHandle) {}

pub fn refresh(_app: &AppHandle) {}
