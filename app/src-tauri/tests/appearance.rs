//! Appearance in the header, per-volume settings, and Benvenuto's list without any file on
//! the stick (scan + installed-mode known list + legacy NiceVault.json migration).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nicevault_app::appearance::Visibility;
use nicevault_app::emblem::Emblem;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use nv_core::{Appearance, LockedEmblem};
use secrecy::SecretString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn app(loc: Locations) -> Arc<AppState> {
    AppState::new(Store::new(loc), Box::new(NoEvents))
}

/// Portable: `exe` is the stick's folder; `usb` = fake removable drive roots; no known list.
fn portable(exe: &Path, usb: &[PathBuf]) -> Locations {
    Locations { exe_dir: Some(exe.to_path_buf()), removable: Some(usb.to_vec()), config_dir: None }
}

fn patch(json: &str) -> nicevault_app::appearance::SettingsPatch {
    serde_json::from_str(json).unwrap()
}

/// Bytes of a PNG header with the given size (only IHDR is ever parsed), padded with `fill`.
fn png(side: u32, len: usize, fill: &[u8]) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&side.to_be_bytes());
    v.extend_from_slice(&side.to_be_bytes());
    while v.len() < len {
        v.extend_from_slice(fill);
    }
    v
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Every file under `dir` (recursive).
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() { out.extend(files(&p)) } else { out.push(p) }
    }
    out
}

fn entry<'a>(list: &'a [nicevault_app::store::RecentEntry], name: &str) -> &'a nicevault_app::store::RecentEntry {
    list.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("{name} not in {list:?}"))
}

/// What an attacker with a hex editor does: same-length replacement in both header copies,
/// unkeyed per-copy checksum fixed up.
fn forge(path: &Path, from: &[u8], to: &[u8]) {
    use std::io::{Seek, SeekFrom, Write};
    let mut raw = std::fs::read(path).unwrap()[..2 * 65536].to_vec();
    for c in 0..2 {
        let copy = &mut raw[c * 65536..(c + 1) * 65536];
        let at = copy.windows(from.len()).position(|w| w == from).unwrap();
        copy[at..at + to.len()].copy_from_slice(to);
        let sum = blake3::hash(&copy[..65536 - 32]);
        copy[65536 - 32..].copy_from_slice(sum.as_bytes());
    }
    let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.write_all(&raw).unwrap();
}

#[test]
fn appearance_visibility_and_settings_per_volume() {
    let tmp = tempfile::tempdir().unwrap();
    let stick = tmp.path();
    let st = app(portable(stick, &[]));
    let casa = stick.join("Casa.nvault");

    // nothing open: session defaults; they seed new volumes
    let d = st.settings_get(None).unwrap();
    assert_eq!((d.accent.as_str(), d.theme.as_str(), d.auto_lock_minutes), ("velluto", "dark", 15));
    // the retired "bordeaux" id reads as its replacement, "velluto"
    assert_eq!(st.settings_set(patch(r#"{"accent": "bordeaux"}"#), None).unwrap().accent, "velluto");
    assert_eq!(st.settings_set(patch(r#"{"accent": "seppia"}"#), None).unwrap().accent, "seppia");
    assert_eq!(st.settings_get(Some("nope")).unwrap_err().code, "locked");

    // new volumes start from the defaults when they don't wear the app's look ("Come l'app" off)
    st.app_settings_set(serde_json::from_str::<nicevault_app::global::AppSettingsPatch>(r#"{"new_like_app": false}"#).unwrap()).unwrap();
    let info = st.vault_create(casa.to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    assert_eq!((info.settings.accent.as_str(), info.visibility, info.show_when_locked), ("seppia", Visibility::Visible, true));
    assert_eq!(serde_json::to_value(&info).unwrap()["visibility"], "visible");
    assert_eq!(
        nv_core::peek_appearance(&casa).unwrap(),
        Some(Appearance { accent: Some("seppia".into()), emblem: Some(LockedEmblem::Initial("C".into())) })
    );
    let r = st.recent_list();
    let e = entry(&r, "Casa");
    assert_eq!((e.accent.as_deref(), e.emblem.clone(), e.show_when_locked), (Some("seppia"), Some(Emblem::Initial), true));

    // image emblem: the full 256 px one stays inside, the 128 px copy goes to the header
    let full = png(256, 30_000, b"FULLIMAGE_w8");
    let small = png(128, 9_000, b"SMALLCOPY_k3");
    let full_b64 = B64.encode(&full);
    let e = st
        .vault_emblem_set(&info.id, Emblem::Image { data_base64: full_b64.clone() }, true, Some(B64.encode(&small)), None)
        .unwrap();
    assert_eq!(e.emblem, Emblem::Image { data_base64: full_b64.clone() });
    let r = st.recent_list();
    assert_eq!(entry(&r, "Casa").emblem, Some(Emblem::Image { data_base64: B64.encode(&small) }));
    let bad = st.vault_emblem_set(&info.id, Emblem::Image { data_base64: full_b64.clone() }, true, Some(full_b64.clone()), None);
    assert_eq!(bad.unwrap_err().code, "invalid_emblem", "a 256 px header copy is refused");

    // settings are per volume; without `vault` they follow the most recently opened one
    let due = st.vault_create(stick.join("Due.nvault").to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    st.settings_set(patch(r#"{"theme": "system", "auto_lock_minutes": 5}"#), None).unwrap();
    assert_eq!(st.settings_get(Some(&due.id)).unwrap().theme, "system");
    assert_eq!(st.settings_get(Some(&info.id)).unwrap().theme, "dark");
    let s = st.settings_set(patch(r#"{"accent": "ottanio", "theme": "light"}"#), Some(&info.id)).unwrap();
    assert_eq!((s.accent.as_str(), s.theme.as_str(), s.auto_lock_minutes), ("ottanio", "light", 15));
    assert_eq!(nv_core::peek_appearance(&casa).unwrap().unwrap().accent.as_deref(), Some("ottanio"), "accent refreshed in the header");
    assert_eq!(st.vault_appearance_get(&info.id).unwrap().accent, "ottanio");

    // encrypted: the header section is cleared; the look survives inside the volume only
    let a = st.vault_appearance_set(&info.id, None, None, Some(Visibility::Encrypted), None, None).unwrap();
    assert_eq!((a.visibility, a.accent.as_str()), (Visibility::Encrypted, "ottanio"));
    assert_eq!(nv_core::peek_appearance(&casa).unwrap(), None);
    let r = st.recent_list();
    let e = entry(&r, "Casa");
    assert_eq!((e.accent.clone(), e.emblem.clone(), e.show_when_locked), (None, None, false));
    st.vault_lock(&info.id, "user").unwrap();
    for f in files(stick) {
        let raw = std::fs::read(&f).unwrap();
        for needle in [&b"ottanio"[..], b"SMALLCOPY_k3", b"FULLIMAGE_w8", b"\x89PNG"] {
            assert!(!contains(&raw, needle), "{} leaks {:?}", f.display(), String::from_utf8_lossy(needle));
        }
    }
    let info = st.vault_open(casa.to_str().unwrap(), pw("p")).unwrap();
    assert_eq!((info.visibility, info.settings.accent.as_str(), info.settings.theme.as_str()), (Visibility::Encrypted, "ottanio", "light"));
    assert_eq!(info.emblem, Emblem::Image { data_base64: full_b64.clone() });
    assert_eq!(nv_core::peek_appearance(&casa).unwrap(), None, "unlock does not bring it back");

    // vault_emblem_set's show_when_locked is the visibility; the small copy was kept
    st.vault_emblem_set(&info.id, Emblem::Image { data_base64: full_b64 }, true, None, None).unwrap();
    let h = nv_core::peek_appearance(&casa).unwrap().unwrap();
    assert_eq!(h, Appearance { accent: Some("ottanio".into()), emblem: Some(LockedEmblem::Image(small)) });

    // edited while locked: listed as forged until the unlock rewrites it from the volume
    st.lock_all("user");
    forge(&casa, b"ottanio", b"prussia");
    assert_eq!(entry(&st.recent_list(), "Casa").accent.as_deref(), Some("prussia"));
    let info = st.vault_open(casa.to_str().unwrap(), pw("p")).unwrap();
    assert!(info.appearance_tampered);
    assert!(st.vault_appearance_get(&info.id).unwrap().tampered);
    assert_eq!(nv_core::peek_appearance(&casa).unwrap().unwrap().accent.as_deref(), Some("ottanio"));
    st.lock_all("user");

    assert!(files(stick).iter().all(|f| f.extension().is_some_and(|x| x == "nvault")), "{:?}", files(stick));
}

#[test]
fn scan_finds_stick_and_removable_volumes_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (stick, usb, elsewhere) = (tmp.path().join("stick"), tmp.path().join("usb"), tmp.path().join("elsewhere"));
    for d in [&stick, &stick.join("Archivio"), &stick.join("a/b"), &usb, &elsewhere] {
        std::fs::create_dir_all(d).unwrap();
    }
    let loc = portable(&stick, std::slice::from_ref(&usb));
    let st = app(loc.clone());
    for p in [stick.join("Uno.nvault"), stick.join("Archivio/Due.nvault"), stick.join("a/b/Profondo.nvault"), usb.join("Tre.nvault"), elsewhere.join("Fuori.nvault")]
    {
        st.vault_create(p.to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    }
    std::fs::write(stick.join("Finto.nvault"), b"not a volume at all").unwrap();
    st.lock_all("user");

    let r = st.recent_list();
    let mut got: Vec<(&str, &str)> = r.iter().map(|e| (e.name.as_str(), e.source)).collect();
    got.sort();
    assert_eq!(got, [("Due", "here"), ("Fuori", "session"), ("Profondo", "session"), ("Tre", "removable"), ("Uno", "here")]);
    assert!(r.iter().all(|e| e.exists && e.unlock_ms.is_some() && e.show_when_locked && e.size_bytes > 0));

    // a new session: only what the scan finds (deeper folders and other places are not looked at)
    let st = app(loc.clone());
    let mut names: Vec<String> = st.recent_list().into_iter().map(|e| e.name).collect();
    names.sort();
    assert_eq!(names, ["Due", "Tre", "Uno"]);
    let e = entry(&st.recent_list(), "Uno").clone();
    assert!(e.last_opened > 0, "mtime as last-opened proxy");
    // forget hides a scanned volume for this session only
    assert!(!st.recent_forget(&e.path).unwrap().iter().any(|x| x.name == "Uno"));
    assert!(app(loc).recent_list().iter().any(|x| x.name == "Uno"));

    let mut all = files(tmp.path());
    all.retain(|f| f.extension().is_none_or(|x| x != "nvault"));
    assert!(all.is_empty(), "nothing but volumes on disk: {all:?}");
}

#[test]
fn installed_mode_remembers_volumes_opened_elsewhere() {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, cfg, docs) = (tmp.path().join("bin"), tmp.path().join("config"), tmp.path().join("docs"));
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&docs).unwrap();
    let loc = Locations { exe_dir: Some(bin.clone()), removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    let st = app(loc.clone());
    let v = docs.join("Tasse.nvault");
    let info = st.vault_create(v.to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    st.vault_emblem_set(&info.id, Emblem::Icon { id: "receipt".into() }, true, None, None).unwrap();
    st.lock_all("user");
    let known = std::fs::read_to_string(cfg.join("known.json")).unwrap();
    assert!(known.contains("Tasse.nvault"));
    assert!(!known.contains("receipt") && !known.contains("prugna"), "only paths and times: {known}");

    let st = app(loc.clone());
    let r = st.recent_list();
    let e = entry(&r, "Tasse");
    assert_eq!((e.source, e.emblem.clone()), ("known", Some(Emblem::Icon { id: "receipt".into() })));
    assert!(e.unlock_ms.is_some());
    st.recent_forget(&e.path).unwrap();
    assert!(!std::fs::read_to_string(cfg.join("known.json")).unwrap().contains("Tasse"));
    assert!(app(loc).recent_list().is_empty());
}

#[test]
fn legacy_json_next_to_the_exe_is_imported_once_and_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let (stick, cfg, far) = (tmp.path().join("stick"), tmp.path().join("config"), tmp.path().join("far"));
    std::fs::create_dir_all(&stick).unwrap();
    std::fs::create_dir_all(&far).unwrap();
    let mk = AppState::new(Store::new(portable(&stick, &[])), Box::new(NoEvents));
    let far_vol = far.join("Lontano.nvault");
    mk.vault_create(far_vol.to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    mk.lock_all("user");
    let legacy = serde_json::json!({
        "recent": [
            { "path": "../far/Lontano.nvault", "name": "Lontano", "last_opened": 1234, "unlock_ms": 640,
              "emblem": { "kind": "icon", "id": "house" } },
            { "path": "/nowhere/Sparito.nvault", "name": "Sparito", "last_opened": 99 }
        ],
        "settings": { "auto_lock_minutes": 5, "accent": "seppia" }
    });
    let write = || std::fs::write(stick.join("NiceVault.json"), serde_json::to_vec(&legacy).unwrap()).unwrap();

    // installed: paths go to the known list in the config dir, settings become the defaults
    write();
    let loc = Locations { exe_dir: Some(stick.clone()), removable: Some(vec![]), config_dir: Some(cfg.clone()) };
    let mut s = Store::new(loc.clone());
    s.migrate_legacy();
    assert!(!stick.join("NiceVault.json").exists(), "deleted");
    assert_eq!((s.defaults().accent.as_str(), s.defaults().auto_lock_minutes), ("seppia", 5));
    let r = s.recent();
    let e = entry(&r, "Lontano");
    assert_eq!((e.source, e.unlock_ms, e.exists), ("known", Some(640), true));
    assert!(!entry(&r, "Sparito").exists);
    let known = std::fs::read_to_string(cfg.join("known.json")).unwrap();
    assert!(known.contains("Lontano") && !known.contains("house"));
    let mut again = Store::new(loc);
    again.migrate_legacy(); // nothing left to import; the list stays
    assert_eq!(again.recent().len(), 2);

    // portable: nothing persisted anywhere, the paths are listed for this session
    std::fs::remove_dir_all(&cfg).unwrap();
    write();
    let mut s = Store::new(portable(&stick, &[]));
    s.migrate_legacy();
    assert!(!stick.join("NiceVault.json").exists());
    assert_eq!(entry(&s.recent(), "Lontano").source, "session");
    assert!(!cfg.exists());
    assert!(Store::new(portable(&stick, &[])).recent().is_empty());
}

/// The ink chosen in Dettagli is what Benvenuto shows: one source of truth (settings -> header).
#[test]
fn chosen_ink_reaches_benvenuto_after_lock() {
    let t = tempfile::tempdir().unwrap();
    let stick = t.path();
    let st = app(portable(stick, &[]));
    let path = stick.join("Verde.nvault");
    let info = st.vault_create(path.to_str().unwrap(), pw("p"), Some("fast")).unwrap();
    assert_eq!(entry(&st.recent_list(), "Verde").accent.as_deref(), Some("velluto"));
    st.settings_set(patch(r#"{"accent": "bottiglia"}"#), Some(&info.id)).unwrap();
    // while it is still open, and after it is locked
    assert_eq!(entry(&st.recent_list(), "Verde").accent.as_deref(), Some("bottiglia"));
    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(entry(&st.recent_list(), "Verde").accent.as_deref(), Some("bottiglia"));
    // and in a new app session reading the file cold
    let st2 = app(portable(stick, &[]));
    assert_eq!(entry(&st2.recent_list(), "Verde").accent.as_deref(), Some("bottiglia"));
}
