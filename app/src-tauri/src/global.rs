use crate::appearance::{Settings, SettingsPatch};
use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, lock_or_recover};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct AppSettings {
    pub language: String,
    pub defaults: Settings,
    pub persisted: bool,
    pub site_icons: bool,
    pub sounds: bool,
    pub video_arrows: String,
    pub sidebar_collapsed: bool,
    pub app_theme: String,
    pub app_accent: String,
    pub ui_scale: u32,
    pub new_like_app: bool,
    pub start_at_login: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
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
    pub portable: bool,
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
