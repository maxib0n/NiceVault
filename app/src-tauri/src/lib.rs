pub mod appearance;
pub mod commands;
pub mod delete;
pub mod device;
pub mod diaglog;
pub mod drives;
pub mod emblem;
pub mod error;
pub mod external;
pub mod fs_events;
pub mod hardening;
pub mod jobs;
pub mod passwords;
pub mod pm_icons;
pub mod preview;
pub mod props;
pub mod resize;
pub mod search;
pub mod state;
pub mod store;
pub mod thumbs;
pub mod updater;
pub mod watch;
pub mod autolock;
pub mod autostart;
pub mod browser;
pub mod global;
pub mod single;
#[cfg(desktop)]
pub mod tray;
pub mod viewers;
pub mod window_geom;
#[cfg(mobile)]
#[path = "tray_mobile.rs"]
pub mod tray;
#[cfg(mobile)]
pub mod mobile;
#[cfg(any(target_os = "android", test))]
pub mod docs;
#[cfg(any(mobile, test))]
pub mod mobile_paths;
#[cfg(any(mobile, test))]
pub mod pick_queue;

pub use state::{AppState, Events, NoEvents};

use std::sync::Arc;
use tauri::{Emitter, Manager};

struct TauriEvents(tauri::AppHandle);

impl Events for TauriEvents {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if event == "vault-opened" || event == "vault-locked" || event == "vault-settings" {
            browser_event(&self.0, event, &payload);
        }
        if event == "vault-opened" || event == "vault-locked" {
            tray::refresh(&self.0);
            #[cfg(mobile)]
            mobile::docs_changed(&self.0);
        }
        let _ = self.0.emit(event, payload);
    }
}

fn browser_event(app: &tauri::AppHandle, event: &str, payload: &serde_json::Value) {
    let (app, opened, vault) = (app.clone(), event == "vault-opened", payload["vault"].as_str().map(str::to_owned));
    std::thread::spawn(move || {
        let Some(b) = app.try_state::<Arc<browser::Browser>>() else { return };
        if !opened {
            return b.notify();
        }
        let path = vault.and_then(|v| {
            let st = app.try_state::<Arc<AppState>>()?;
            let g = state::lock_or_recover(&st.vaults);
            g.get(&v).map(|o| o.path.to_string_lossy().into_owned())
        });
        b.opened(path.as_deref());
    });
}

struct BrowserHost(tauri::AppHandle);

impl browser::Host for BrowserHost {
    fn pair_prompt(&self, p: &browser::PairPrompt) {
        #[cfg(desktop)]
        tray::show_window(&self.0);
        let _ = self.0.emit("browser-pair", p);
    }
    fn pair_closed(&self, id: &str) {
        let _ = self.0.emit("browser-pair-closed", serde_json::json!({ "id": id }));
    }
    fn unlock_window(&self, path: &str) {
        #[cfg(desktop)]
        {
            let (h, path) = (self.0.clone(), path.to_owned());
            let _ = self.0.run_on_main_thread(move || compact_unlock(&h, &path));
        }
    }
    fn unlock_done(&self) {
        let h = self.0.clone();
        let _ = self.0.run_on_main_thread(move || {
            if let Some(w) = h.get_webview_window(UNLOCK_WINDOW) {
                let _ = w.close();
            }
        });
    }
    fn clipboard(&self) -> Option<Arc<dyn passwords::Clipboard>> {
        Some(Arc::new(passwords::TauriClipboard(self.0.clone())))
    }
    fn changed(&self) {
        let _ = self.0.emit("browser-changed", ());
    }
    fn show_app(&self) {
        #[cfg(desktop)]
        tray::show_window(&self.0);
    }
}

const UNLOCK_WINDOW: &str = "unlock";

#[cfg(desktop)]
fn compact_unlock(app: &tauri::AppHandle, path: &str) {
    let spec = serde_json::json!({ "kind": "unlock", "path": path });
    if let Some(w) = app.get_webview_window(UNLOCK_WINDOW) {
        let _ = w.eval(format!("window.__nvCompactSet && window.__nvCompactSet({spec})"));
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let b = tauri::WebviewWindowBuilder::new(app, UNLOCK_WINDOW, tauri::WebviewUrl::App("index.html".into()))
        .title("NiceVault")
        .inner_size(440.0, 360.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .always_on_top(true)
        .center()
        .focused(true)
        .background_color(tauri::window::Color(0x1b, 0x1a, 0x19, 0xff))
        .initialization_script(format!("window.__NV_COMPACT__ = {spec};"));
    if let Ok(w) = b.build() {
        let _ = w.set_focus();
    }
}

fn app_url(u: &tauri::Url) -> bool {
    let dev_url = cfg!(debug_assertions);
    match (u.scheme(), u.host_str()) {
        ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost")) => true,
        ("http", Some("localhost")) => dev_url,
        _ => false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let minimized = autostart::launched_minimized(std::env::args_os());
    let background = std::env::args_os().skip(1).any(|a| a == "--background");
    let minimized = minimized || background;
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    if single::enabled() {
        builder = builder.plugin(tauri_plugin_single_instance::init(single::on_second));
    }
    #[cfg(mobile)]
    {
        builder = builder.plugin(tauri_plugin_fs::init()).plugin(mobile::plugin());
    }
    #[cfg(target_os = "ios")]
    {
        builder = builder.on_web_content_process_terminate(|w| mobile::web_content_gone(w.clone()));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri::plugin::Builder::<tauri::Wry>::new("nav-guard").on_navigation(|_, url| app_url(url)).build())
        .register_asynchronous_uri_scheme_protocol("nvault", |ctx, request, responder| {
            let state = ctx.app_handle().state::<Arc<AppState>>().inner().clone();
            std::thread::spawn(move || responder.respond(preview::handle(&state, &request)));
        })
        .on_window_event(|window, event| {
            #[cfg(desktop)]
            match event {
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => window_geom::track(window),
                tauri::WindowEvent::Destroyed if window.label() == UNLOCK_WINDOW => {
                    if let Some(b) = window.app_handle().try_state::<Arc<browser::Browser>>() {
                        b.unlock_cancelled();
                    }
                }
                tauri::WindowEvent::CloseRequested { .. } if window.label() == "main" => {
                    window_geom::track(window);
                    if let Some(k) = window.app_handle().try_state::<window_geom::Keeper>() {
                        k.persist();
                    }
                }
                _ => {}
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
                && tray::hides_on_close(window.app_handle())
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            #[cfg(mobile)]
            mobile::prepare(app.handle());
            std::thread::spawn(|| {
                external::cleanup_stale(&external::temp_root());
                external::cleanup_stale(&std::env::temp_dir());
            });
            let state = AppState::new(store::Store::open_default(), Box::new(TauriEvents(app.handle().clone())));
            let config_dir = state::lock_or_recover(&state.store).locations().config_dir.clone();
            app.manage(Arc::new(updater::Updater::for_app(config_dir.clone())));
            watch::spawn_sleep_watcher(Arc::downgrade(&state));
            autolock::spawn_ticker(Arc::downgrade(&state));
            let scale = state::lock_or_recover(&state.store).app_look().2;
            if scale != 100
                && cfg!(desktop)
                && let Some(w) = app.get_webview_window("main")
            {
                let _ = w.set_zoom(f64::from(scale) / 100.0);
            }
            let drives = drives::Drives::new(drives::Source::detect());
            drives.spawn(Arc::downgrade(&state));
            app.manage(drives);
            let portable = config_dir.is_none();
            let log_dir = if portable { std::env::temp_dir().join("NiceVault") } else { dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("NiceVault") };
            app.manage(device::job::DeviceCtx::new(device::backend::detect(), log_dir));
            app.manage(state);
            let host = Box::new(BrowserHost(app.handle().clone()));
            let br = browser::Browser::new(Arc::downgrade(app.state::<Arc<AppState>>().inner()), config_dir.clone(), host);
            #[cfg(desktop)]
            {
                browser::listen(&br);
                browser::sync_registration(&br);
            }
            app.manage(br);
            app.manage(Arc::new(viewers::MediaServer::default()));
            let launch = single::Launch::default();
            if let Ok(cwd) = std::env::current_dir()
                && let Some(p) = single::volume_arg(std::env::args(), &cwd)
            {
                launch.put(p);
            }
            app.manage(launch);
            app.manage(tray::Tray::new(minimized));
            tray::sync(app.handle());
            if let Some(w) = app.get_webview_window("main") {
                #[cfg(desktop)]
                {
                    let keeper = window_geom::Keeper::new(config_dir.clone());
                    window_geom::apply(&w, &keeper);
                    app.manage(keeper);
                }
                let tray_up = app.state::<tray::Tray>().active();
                if !minimized || !tray_up {
                    let _ = w.show();
                }
                #[cfg(desktop)]
                if minimized && !tray_up && !background {
                    let _ = w.minimize();
                }
                if minimized && tray_up {
                    tray::show_unless_hosted(app.handle());
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::vault_create,
            commands::vault_open,
            commands::vault_lock,
            commands::vault_list_open,
            commands::vault_stats,
            commands::vault_emblem_get,
            commands::vault_emblem_set,
            commands::vault_appearance_get,
            commands::vault_appearance_set,
            commands::recent_list,
            commands::recent_forget,
            commands::default_volume_dir,
            commands::settings_get,
            commands::settings_set,
            search::cmd::fs_list,
            commands::fs_mkdir,
            commands::fs_rename,
            commands::fs_delete,
            commands::import_size,
            #[cfg(desktop)]
            commands::fs_import,
            #[cfg(mobile)]
            mobile::fs_import,
            #[cfg(mobile)]
            mobile::mobile_volume_in,
            #[cfg(mobile)]
            mobile::mobile_share,
            #[cfg(mobile)]
            mobile::mobile_export_dir,
            #[cfg(mobile)]
            mobile::mobile_import_photos,
            #[cfg(mobile)]
            mobile::mobile_pick_files,
            #[cfg(mobile)]
            mobile::mobile_save_gallery,
            #[cfg(mobile)]
            mobile::mobile_docs_busy,
            commands::fs_export,
            commands::fs_open_external,
            commands::job_cancel,
            commands::mount_available,
            commands::mount,
            commands::unmount,
            commands::slots_list,
            commands::slot_add_password,
            commands::slot_change_password,
            commands::slot_remove,
            commands::slot_rotate_key,
            commands::vault_upgrade_format,
            commands::vault_accept_rollback,
            passwords::cmd::pm_list,
            passwords::cmd::pm_search,
            passwords::cmd::pm_get,
            passwords::cmd::pm_create,
            passwords::cmd::pm_update,
            passwords::cmd::pm_delete,
            passwords::cmd::pm_history,
            passwords::cmd::pm_reveal,
            passwords::cmd::pm_copy,
            passwords::cmd::pm_totp,
            passwords::cmd::pm_totp_check,
            passwords::cmd::pm_generate,
            passwords::cmd::pm_import,
            passwords::cmd::pm_move,
            passwords::cmd::pm_folder_rename,
            passwords::cmd::pm_folder_delete,
            passwords::cmd::pm_repair,
            passwords::cmd::pm_icons_refresh,
            commands::vault_touch,
            commands::autolock_status,
            commands::slot_add_duress,
            commands::app_settings_get,
            commands::app_settings_set,
            commands::app_info,
            browser::cmd::browser_info,
            browser::cmd::browser_set,
            browser::cmd::browser_unpair,
            browser::cmd::browser_pair_answer,
            browser::cmd::browser_unlock_cancel,
            single::cmd::launch_path_take,
            updater::cmd::update_check,
            updater::cmd::update_download,
            updater::cmd::update_install,
            updater::cmd::update_restart,
            updater::cmd::update_prefs_get,
            updater::cmd::update_prefs_set,
            drives::cmd::drives_list,
            device::cmd::device_candidates,
            device::cmd::device_options,
            device::cmd::device_plan,
            device::cmd::device_create,
            device::cmd::device_resume,
            device::cmd::device_eject,
            device::cmd::device_fetch_missing,
            device::cmd::device_log,
            device::cmd::device_outcome,
            device::cmd::device_open,
            device::cmd::device_sticks,
            device::cmd::device_stick_mount,
            device::cmd::device_stick_update,
            props::cmd::fs_props,
            props::cmd::fs_props_tree,
            props::cmd::fs_hash,
            search::cmd::fs_search,
            search::cmd::tags_list,
            search::cmd::tags_create,
            search::cmd::tags_update,
            search::cmd::tags_delete,
            search::cmd::tags_assign,
            search::cmd::tags_unassign,
            search::cmd::tags_of,
            viewers::cmd::media_url,
            viewers::cmd::media_release,
            viewers::cmd::fs_open_mounted,
            viewers::cmd::open_link,
            resize::cmd::vault_resize_info,
            resize::cmd::vault_resize,
        ])
        .build(tauri::generate_context!())
        .expect("error while building NiceVault");

    app.run(|handle, event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            tray::show_window(handle);
        }
        if let tauri::RunEvent::Exit = event {
            if let Some(k) = handle.try_state::<window_geom::Keeper>() {
                k.persist();
            }
            if let Some(state) = handle.try_state::<Arc<AppState>>() {
                state.lock_all("user");
            }
            watch::shutdown();
            drives::shutdown();
        }
    });
}
