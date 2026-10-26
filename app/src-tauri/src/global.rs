//! Global settings (not per volume): the UI language, the app's own appearance (theme, ink,
//! interface size: worn wherever no volume's look applies) and the defaults for NEW volumes, plus
//! what the Impostazioni screen shows about the app, and how it starts ("Avvia all'accesso",
//! "Avvia ridotto", "Resta nel vassoio": installed mode only, see autostart.rs and tray.rs). Stored by `store.rs`: installed mode in
//! `settings.json` in the OS config dir, portable mode in memory for this session only.

use crate::appearance::{Settings, SettingsPatch};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, lock_or_recover};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct AppSettings {
    /// "system" | "it" | "en"
    pub language: String,
    /// What a volume created from now on starts with (theme, accent, auto-lock options).
    pub defaults: Settings,
    /// true = kept in the OS config dir (installed); false = this session only (portable).
    pub persisted: bool,
    /// Password manager: fetch each entry's site icon from its website (default on).
    pub site_icons: bool,
    /// Subtle UI sounds (default on).
    pub sounds: bool,
    /// ←/→ in a full-screen video: "seek" (default) or "step" (previous / next file).
    pub video_arrows: String,
    /// The volume's sidebar folded to a slim rail (desktop; default off).
    pub sidebar_collapsed: bool,
    /// The app's own theme ("dark" | "light" | "system") and ink: Benvenuto, Impostazioni, the
    /// wizard, Sblocca of a volume whose look is encrypted. A volume's own look wins inside it.
    pub app_theme: String,
    pub app_accent: String,
    /// Interface size, percent (one of `store::UI_SCALES`): the webview's zoom.
    pub ui_scale: u32,
    /// New volumes start with the app's theme and ink ("Come l'app"), not `defaults.theme / accent`.
    pub new_like_app: bool,
    /// "Avvia all'accesso": the OS starts NiceVault when this user logs in (installed only).
    pub start_at_login: bool,
    /// "Avvia ridotto": started at login it stays hidden, with a tray icon; closing the window
    /// hides it there instead of quitting (installed only).
    pub start_minimized: bool,
    /// "Resta nel vassoio quando chiudi la finestra": closing the window hides it, NiceVault
    /// keeps running in the tray (volumes open, auto-lock on); the tray's Esci quits. Installed
    /// only (default on); without a tray on this desktop closing quits anyway (tray.rs).
    pub close_to_tray: bool,
    /// Phones: "Salva foto e video nella Galleria" — Esporta puts pictures and videos in the
    /// system gallery (iOS Photos, Android Pictures/Movies) instead of Files. null = never asked:
    /// the first export of a photo asks once.
    pub gallery_save: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AppSettingsPatch {
    pub language: Option<String>,
    pub defaults: Option<SettingsPatch>,
    pub site_icons: Option<bool>,
    pub sounds: Option<bool>,
    pub video_arrows: Option<String>,
    pub sidebar_collapsed: Option<bool>,
    pub app_theme: Option<String>,
    pub app_accent: Option<String>,
    pub ui_scale: Option<u32>,
    pub new_like_app: Option<bool>,
    pub start_at_login: Option<bool>,
    pub start_minimized: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub gallery_save: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppInfo {
    pub version: String,
    /// Running from a removable drive: nothing is written on the host or the drive.
    pub portable: bool,
    /// Where installed mode keeps known.json and settings.json.
    pub config_dir: Option<String>,
}

impl AppState {
    pub fn app_settings_get(&self) -> AppSettings {
        let s = lock_or_recover(&self.store);
        let (app_theme, app_accent, ui_scale, new_like_app) = s.app_look();
        let (start_at_login, start_minimized) = s.startup();
        AppSettings {
            language: s.language(),
            defaults: s.defaults(),
            persisted: s.installed(),
            app_theme,
            app_accent,
            ui_scale,
            new_like_app,
            site_icons: s.site_icons(),
            sounds: s.sounds(),
            video_arrows: s.video_arrows().to_owned(),
            sidebar_collapsed: s.sidebar_collapsed(),
            start_at_login,
            start_minimized,
            close_to_tray: s.close_to_tray(),
            gallery_save: s.gallery_save(),
        }
    }

    pub fn app_settings_set(&self, patch: AppSettingsPatch) -> ApiResult<AppSettings> {
        {
            let mut s = lock_or_recover(&self.store);
            if s.installed() && s.global_unreadable() {
                return Err(ApiError::new(
                    crate::store::GLOBAL_UNREADABLE,
                    "settings.json was written by a newer NiceVault (or is damaged): it is left as it is.",
                ));
            }
            if let Some(l) = patch.language.as_deref() {
                s.set_language(l);
            }
            if let Some(d) = patch.defaults {
                s.set_defaults(d);
            }
            if let Some(b) = patch.site_icons {
                s.set_site_icons(b);
            }
            if let Some(b) = patch.sounds {
                s.set_sounds(b);
            }
            if let Some(v) = patch.video_arrows.as_deref() {
                s.set_video_arrows(v);
            }
            if let Some(b) = patch.sidebar_collapsed {
                s.set_sidebar_collapsed(b);
            }
            if patch.app_theme.is_some() || patch.app_accent.is_some() || patch.ui_scale.is_some() || patch.new_like_app.is_some() {
                s.set_app_look(patch.app_theme.as_deref(), patch.app_accent.as_deref(), patch.ui_scale, patch.new_like_app);
            }
            if patch.start_at_login.is_some() || patch.start_minimized.is_some() {
                s.set_startup(patch.start_at_login, patch.start_minimized)
                    .map_err(|e| ApiError::new("autostart_failed", format!("The start-at-login entry could not be written: {e}")))?;
            }
            if let Some(b) = patch.close_to_tray {
                s.set_close_to_tray(b);
            }
            if let Some(b) = patch.gallery_save {
                s.set_gallery_save(b);
            }
        }
        Ok(self.app_settings_get())
    }

    pub fn app_info(&self) -> AppInfo {
        let s = lock_or_recover(&self.store);
        AppInfo {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            portable: !s.installed(),
            config_dir: s.locations().config_dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
        }
    }
}
