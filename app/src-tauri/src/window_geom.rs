//! The main window's size and place (it opens fairly small, never larger than the screen).
//!
//! First start: about 80 % of the work area of the screen it opens on (logical px, so a 4K panel
//! at 200 % gets the same proportion as a 1080p one), at least the window's minimum (900 x 600,
//! tauri.conf.json), at most 1680 x 1050, centred. After that, the size and place it had when it
//! was last closed: kept in `window.json` next to `settings.json` when installed, nothing kept
//! when portable (nothing on the stick, nothing on the host; within a session the window keeps
//! its own geometry anyway). A saved place is used only if the window's title bar would land on
//! a screen that is there now (a monitor unplugged since, a smaller desk): never off screen.
//! Wayland does not let a client place its window: the size is applied, the place is the
//! compositor's.
// (Android / iOS: one full-screen window, nothing to size or keep)
#![cfg_attr(mobile, allow(dead_code))]
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const FILE: &str = "window.json";
/// tauri.conf.json minWidth / minHeight
pub const MIN: (f64, f64) = (900.0, 600.0);
/// the first size is never larger than this (logical px): a book on a table, not a wall
pub const MAX_FIRST: (f64, f64) = (1680.0, 1050.0);
pub const FIRST_SHARE: f64 = 0.8;

/// What is kept: the normal (not maximised) frame, position in physical px of the virtual
/// desktop, size in logical px (survives a scale change), and whether it was maximised.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub x: i32,
    pub y: i32,
    pub w: f64,
    pub h: f64,
    #[serde(default)]
    pub maximized: bool,
}

/// A screen's work area (physical px) and its scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub scale: f64,
}

impl Screen {
    fn logical(&self) -> (f64, f64) {
        (f64::from(self.w) / self.scale, f64::from(self.h) / self.scale)
    }
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && i64::from(px) < i64::from(self.x) + i64::from(self.w) && i64::from(py) < i64::from(self.y) + i64::from(self.h)
    }
}

/// Where and how large to open: size in logical px, position (physical) when it is ours to set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub w: f64,
    pub h: f64,
    pub pos: Option<(i32, i32)>,
    pub maximized: bool,
}

/// The size that fits on `s`: no larger than its work area, no smaller than the minimum
/// (unless the work area itself is smaller).
fn fit(s: &Screen, w: f64, h: f64) -> (f64, f64) {
    let (aw, ah) = s.logical();
    (w.min(aw).max(MIN.0.min(aw)).round(), h.min(ah).max(MIN.1.min(ah)).round())
}

/// Top-left (physical) that centres a logical `w` x `h` on `s`.
fn centre(s: &Screen, w: f64, h: f64) -> (i32, i32) {
    let pw = (w * s.scale).round() as i64;
    let ph = (h * s.scale).round() as i64;
    let x = i64::from(s.x) + (i64::from(s.w) - pw).max(0) / 2;
    let y = i64::from(s.y) + (i64::from(s.h) - ph).max(0) / 2;
    (x as i32, y as i32)
}

/// The first size on `s`: 80 % of its work area, within [MIN, MAX_FIRST], centred.
pub fn first(s: &Screen) -> Plan {
    let (aw, ah) = s.logical();
    let (w, h) = fit(s, (aw * FIRST_SHARE).min(MAX_FIRST.0), (ah * FIRST_SHARE).min(MAX_FIRST.1));
    Plan { w, h, pos: Some(centre(s, w, h)), maximized: false }
}

/// The plan for this start: the saved frame when its title bar is on a screen that is there now
/// (resized to fit that screen and pulled fully onto it), else the first size on `home`
/// (the screen the window opens on). `None` only without any screen at all.
pub fn plan(saved: Option<Saved>, screens: &[Screen], home: Option<usize>) -> Option<Plan> {
    let home = home.and_then(|i| screens.get(i)).or_else(|| screens.first())?;
    let Some(sv) = saved.filter(|s| s.w.is_finite() && s.h.is_finite() && s.w > 0.0 && s.h > 0.0) else {
        return Some(first(home));
    };
    // the title bar: a point a little in from the top-left and one in the middle of the top edge
    let on = screens.iter().find(|s| {
        let tw = (sv.w * s.scale) as i64;
        let ty = i64::from(sv.y) + (16.0 * s.scale) as i64;
        let mid = i64::from(sv.x) + tw / 2;
        let left = i64::from(sv.x) + (40.0 * s.scale) as i64;
        [mid, left].iter().any(|&px| i32::try_from(px).ok().zip(i32::try_from(ty).ok()).is_some_and(|(px, py)| s.contains(px, py)))
    });
    let Some(s) = on else {
        // gone, or off every screen: a fresh start on the home screen (its size if it fits there)
        let (w, h) = fit(home, sv.w, sv.h);
        return Some(Plan { w, h, pos: Some(centre(home, w, h)), maximized: sv.maximized });
    };
    let (w, h) = fit(s, sv.w, sv.h);
    let pw = (w * s.scale).round() as i64;
    let ph = (h * s.scale).round() as i64;
    let max_x = i64::from(s.x) + i64::from(s.w) - pw;
    let max_y = i64::from(s.y) + i64::from(s.h) - ph;
    let x = i64::from(sv.x).min(max_x).max(i64::from(s.x));
    let y = i64::from(sv.y).min(max_y).max(i64::from(s.y));
    Some(Plan { w, h, pos: Some((x as i32, y as i32)), maximized: sv.maximized })
}

pub fn load(dir: &Path) -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(dir.join(FILE)).ok()?).ok()
}

pub fn save(dir: &Path, s: &Saved) {
    if let Ok(json) = serde_json::to_vec_pretty(s) {
        let _ = crate::store::write_private(&dir.join(FILE), &json);
    }
}

/// The window's last normal frame this session, and where it is kept (installed only).
pub struct Keeper {
    dir: Option<PathBuf>,
    last: Mutex<Option<Saved>>,
}

impl Keeper {
    pub fn new(dir: Option<PathBuf>) -> Self {
        let last = dir.as_deref().and_then(load);
        Keeper { dir, last: Mutex::new(last) }
    }
    pub fn saved(&self) -> Option<Saved> {
        *self.last.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// The window moved or was resized: its normal frame (a maximised or minimised window keeps the
    /// frame it will come back to, only the flag changes).
    pub fn note(&self, frame: Option<(i32, i32, f64, f64)>, maximized: bool) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        match (frame, last.as_mut()) {
            (Some((x, y, w, h)), _) if !maximized => *last = Some(Saved { x, y, w, h, maximized: false }),
            (_, Some(s)) => s.maximized = maximized,
            _ => {}
        }
    }
    /// Written when the window closes / hides and when the app quits (installed only).
    pub fn persist(&self) {
        if let (Some(dir), Some(s)) = (&self.dir, self.saved()) {
            save(dir, &s);
        }
    }
}

#[cfg(desktop)]
mod tauri_side {
    use super::*;
    use tauri::{LogicalSize, Manager, PhysicalPosition, WebviewWindow};

    fn screens(w: &WebviewWindow) -> (Vec<Screen>, Option<usize>) {
        let mons = w.available_monitors().unwrap_or_default();
        let list: Vec<Screen> = mons
            .iter()
            .map(|m| {
                let a = m.work_area();
                // a work area the platform could not tell: the whole monitor
                let (pos, size) = if a.size.width > 0 && a.size.height > 0 { (a.position, a.size) } else { (*m.position(), *m.size()) };
                Screen { x: pos.x, y: pos.y, w: size.width, h: size.height, scale: m.scale_factor().max(0.5) }
            })
            .collect();
        let home = w.current_monitor().ok().flatten().or_else(|| w.primary_monitor().ok().flatten());
        let idx = home.and_then(|h| mons.iter().position(|m| m.position() == h.position() && m.size() == h.size()));
        (list, idx)
    }

    /// Sizes and places the (still hidden) main window before it is shown.
    pub fn apply(w: &WebviewWindow, keeper: &Keeper) {
        let (list, home) = screens(w);
        let Some(p) = plan(keeper.saved(), &list, home) else { return };
        let _ = w.set_size(LogicalSize::new(p.w, p.h));
        // (Wayland: the compositor places the window; asking is harmless)
        if let Some((x, y)) = p.pos {
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
        if p.maximized {
            let _ = w.maximize();
        }
    }

    /// Moved / Resized: remember the frame (not while minimised: its place is off screen then).
    pub fn track(w: &tauri::Window) {
        if w.label() != "main" || w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true) {
            return;
        }
        let Some(keeper) = w.app_handle().try_state::<Keeper>() else { return };
        let maximized = w.is_maximized().unwrap_or(false);
        let frame = (|| {
            let pos = w.outer_position().ok()?;
            let size = w.inner_size().ok()?;
            let scale = w.scale_factor().ok()?;
            Some((pos.x, pos.y, f64::from(size.width) / scale, f64::from(size.height) / scale))
        })();
        keeper.note(frame, maximized);
    }
}
#[cfg(desktop)]
pub use tauri_side::{apply, track};

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: Screen = Screen { x: 0, y: 0, w: 1920, h: 1040, scale: 1.0 };
    const QHD_RIGHT: Screen = Screen { x: 1920, y: 0, w: 2560, h: 1400, scale: 1.0 };
    const UHD_200: Screen = Screen { x: 0, y: 0, w: 3840, h: 2120, scale: 2.0 };

    #[test]
    fn first_start_is_most_of_the_screen_centred() {
        let p = first(&FHD);
        assert_eq!((p.w, p.h), (1536.0, 832.0));
        assert_eq!(p.pos, Some((192, 104)));
        // capped on a large panel
        let p = first(&QHD_RIGHT);
        assert_eq!((p.w, p.h), (1680.0, 1050.0));
        assert_eq!(p.pos, Some((1920 + 440, 175)));
        // the scale counts: 3840x2120 at 200 % is a 1920x1060 logical desk
        let p = first(&UHD_200);
        assert_eq!((p.w, p.h), (1536.0, 848.0));
        assert_eq!(p.pos, Some((384, 212)));
    }

    #[test]
    fn small_screens_get_the_minimum_or_the_whole_screen() {
        let laptop = Screen { x: 0, y: 0, w: 1366, h: 728, scale: 1.0 };
        let p = first(&laptop);
        assert_eq!((p.w, p.h), (1093.0, 600.0)); // 80 % of 728 is under the minimum height
        let tiny = Screen { x: 0, y: 0, w: 800, h: 560, scale: 1.0 };
        let p = first(&tiny);
        assert_eq!((p.w, p.h), (800.0, 560.0));
        assert_eq!(p.pos, Some((0, 0)));
    }

    #[test]
    fn saved_frame_comes_back_where_it_was() {
        let s = Saved { x: 2000, y: 100, w: 1400.0, h: 900.0, maximized: false };
        let p = plan(Some(s), &[FHD, QHD_RIGHT], Some(0)).unwrap();
        assert_eq!(p, Plan { w: 1400.0, h: 900.0, pos: Some((2000, 100)), maximized: false });
        let m = Saved { maximized: true, ..s };
        assert!(plan(Some(m), &[FHD, QHD_RIGHT], Some(0)).unwrap().maximized);
    }

    #[test]
    fn never_off_screen() {
        // it was on the monitor on the right, unplugged since: centred on the one there is
        let s = Saved { x: 2000, y: 100, w: 1400.0, h: 900.0, maximized: false };
        let p = plan(Some(s), &[FHD], Some(0)).unwrap();
        assert_eq!((p.w, p.h), (1400.0, 900.0));
        assert_eq!(p.pos, Some((260, 70)));
        // hanging off the bottom-right: pulled fully onto the screen
        let s = Saved { x: 1500, y: 700, w: 1200.0, h: 780.0, maximized: false };
        let p = plan(Some(s), &[FHD], Some(0)).unwrap();
        assert_eq!(p.pos, Some((720, 260)));
        // larger than the screen it is on now: shrunk to it
        let s = Saved { x: 0, y: 0, w: 2500.0, h: 1400.0, maximized: false };
        let p = plan(Some(s), &[FHD], Some(0)).unwrap();
        assert_eq!((p.w, p.h, p.pos), (1920.0, 1040.0, Some((0, 0))));
        // above the top: the title bar would be unreachable
        let s = Saved { x: 100, y: -500, w: 1200.0, h: 780.0, maximized: false };
        let p = plan(Some(s), &[FHD], Some(0)).unwrap();
        assert_eq!(p.pos, Some((360, 130)));
        // garbage in the file
        let s = Saved { x: 0, y: 0, w: f64::NAN, h: 0.0, maximized: false };
        assert_eq!(plan(Some(s), &[FHD], Some(0)), Some(first(&FHD)));
        assert_eq!(plan(None, &[], None), None);
    }

    #[test]
    fn keeper_remembers_the_normal_frame_and_writes_when_installed() {
        let t = tempfile::tempdir().unwrap();
        let k = Keeper::new(Some(t.path().to_path_buf()));
        assert_eq!(k.saved(), None);
        k.note(Some((10, 20, 1300.0, 850.0)), false);
        // maximised: the frame to come back to stays, the flag is kept
        k.note(Some((0, 0, 1920.0, 1040.0)), true);
        assert_eq!(k.saved(), Some(Saved { x: 10, y: 20, w: 1300.0, h: 850.0, maximized: true }));
        k.persist();
        assert_eq!(load(t.path()), k.saved());
        assert_eq!(Keeper::new(Some(t.path().to_path_buf())).saved(), k.saved());
        // portable: nothing written anywhere
        let p = Keeper::new(None);
        p.note(Some((1, 2, 1000.0, 700.0)), false);
        p.persist();
        assert!(p.saved().is_some());
    }
}
