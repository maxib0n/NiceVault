//! Password manager end to end through the same AppState methods the `pm_*` commands call:
//! create -> list/get (no secrets) -> reveal -> copy + clipboard auto-clear -> TOTP -> generator
//! -> imports (Bitwarden file, CSV data, KeePass base64 with wrong/right password) -> lock -> reopen.

use nicevault_app::passwords::Clipboard;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_core::passwords::EntryPatch;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<(String, Value)>>>);

impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

impl Rec {
    fn job_done(&self, job: &str) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().find(|(e, v)| e == "job-done" && v["job"] == job) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "no job-done for {job}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn progress(&self, job: &str) -> Vec<Value> {
        self.0.lock().unwrap().iter().filter(|(e, v)| e == "job-progress" && v["job"] == job).map(|(_, v)| v.clone()).collect()
    }
}

#[derive(Default)]
struct FakeClip(Mutex<Option<String>>);

impl Clipboard for FakeClip {
    fn read(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn write(&self, text: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(text.to_owned());
        Ok(())
    }
    fn clear(&self) {
        *self.0.lock().unwrap() = Some(String::new());
    }
}

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/nv-core/tests/fixtures").join(name)
}

fn patch(v: Value) -> EntryPatch {
    serde_json::from_value(v).unwrap()
}

#[test]
fn password_manager_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = Rec::default();
    let st = AppState::new(
        Store::new(Locations { exe_dir: Some(tmp.path().to_path_buf()), removable: Some(vec![]), config_dir: None }),
        Box::new(rec.clone()),
    );
    let vol = tmp.path().join("Chiavi.nvault");
    let id = st.vault_create(vol.to_str().unwrap(), pw("segreto"), Some("fast")).unwrap().id;

    // create (the UI sends exactly this JSON shape)
    let e = st
        .pm_create(
            &id,
            patch(json!({
                "title": "Posta",
                "username": "anna@example.com",
                "password": "Pw-Segreta-123",
                "urls": ["https://mail.example.com"],
                "notes": "nota",
                "totp": "otpauth://totp/Posta:anna?secret=JBSWY3DPEHPK3PXP&issuer=Posta",
                "folder": "Lavoro",
                "tags": ["email"],
                "favorite": true,
                "fields": [{ "name": "PIN", "value": "9876", "hidden": true }, { "name": "Piano", "value": "3", "hidden": false }]
            })),
        )
        .unwrap();
    let eid = e.summary.id.clone();
    let shown = serde_json::to_string(&e).unwrap() + &serde_json::to_string(&st.pm_list(&id).unwrap()).unwrap();
    for secret in ["Pw-Segreta-123", "9876", "JBSWY3DP"] {
        assert!(!shown.contains(secret), "{secret} leaked into list/get: {shown}");
    }
    let detail = serde_json::to_value(st.pm_get(&id, &eid).unwrap()).unwrap();
    assert_eq!(detail["title"], "Posta");
    assert_eq!(detail["has_totp"], true);
    assert_eq!(detail["fields"][0], json!({ "name": "PIN", "hidden": true, "value": null }));
    assert_eq!(detail["totp"]["issuer"], "Posta");
    assert_eq!(st.pm_search(&id, "mail lavoro").unwrap().len(), 1);
    assert!(st.pm_search(&id, "nessuno").unwrap().is_empty());

    // reveal
    assert_eq!(&*st.pm_reveal(&id, &eid, "password").unwrap(), "Pw-Segreta-123");
    assert_eq!(&*st.pm_reveal(&id, &eid, "field:0").unwrap(), "9876");
    assert_eq!(st.pm_reveal(&id, &eid, "bogus").unwrap_err().code, "invalid_field");
    assert_eq!(st.pm_reveal(&id, "missing", "password").unwrap_err().code, "not_found");
    assert_eq!(st.pm_create(&id, patch(json!({ "title": "x", "totp": "otpauth://hotp/x?secret=AA" }))).unwrap_err().code, "invalid_totp");
    assert_eq!(st.pm_create(&id, patch(json!({ "title": " " }))).unwrap_err().code, "invalid_entry");

    // copy: cleared after the delay only while the clipboard still holds the value
    let clip = Arc::new(FakeClip::default());
    let c = st.pm_copy(clip.clone(), &id, &eid, "password", Some(1)).unwrap();
    assert_eq!(c.clear_after, 1);
    assert_eq!(clip.read().as_deref(), Some("Pw-Segreta-123"));
    std::thread::sleep(Duration::from_millis(1600));
    assert_eq!(clip.read().as_deref(), Some(""), "cleared");
    st.pm_copy(clip.clone(), &id, &eid, "username", Some(1)).unwrap();
    clip.write("something the user copied later").unwrap();
    std::thread::sleep(Duration::from_millis(1600));
    assert_eq!(clip.read().as_deref(), Some("something the user copied later"), "not ours: left alone");
    assert_eq!(st.pm_copy(clip.clone(), &id, &eid, "totp_code", None).unwrap().clear_after, 30);
    assert_eq!(clip.read().unwrap().len(), 6);

    // TOTP
    let code = serde_json::to_value(st.pm_totp(&id, &eid).unwrap()).unwrap();
    assert_eq!(code["code"].as_str().unwrap().len(), 6);
    assert_eq!(code["period"], 30);
    let chk = serde_json::to_value(st.pm_totp_check("JBSWY3DPEHPK3PXP").unwrap()).unwrap();
    assert_eq!((chk["digits"].as_u64(), chk["algorithm"].as_str()), (Some(6), Some("SHA1")));
    assert_eq!(st.pm_totp_check("nope://").unwrap_err().code, "invalid_totp");

    // update + history
    st.pm_update(&id, &eid, patch(json!({ "password": "Nuova-456", "fields": [{ "name": "PIN", "hidden": true }] }))).unwrap();
    let h = st.pm_history(&id, &eid).unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(&*st.pm_reveal(&id, &eid, "history:0").unwrap(), "Pw-Segreta-123");
    assert_eq!(&*st.pm_reveal(&id, &eid, "field:0").unwrap(), "9876", "kept with value: null");

    // generator
    let g = st.pm_generate(serde_json::from_value(json!({ "kind": "random", "length": 24 })).unwrap()).unwrap();
    assert_eq!(g.value.chars().count(), 24);
    assert!(g.entropy_bits > 140.0);
    let g = st.pm_generate(serde_json::from_value(json!({ "kind": "passphrase", "words": 4, "language": "it" })).unwrap()).unwrap();
    assert_eq!(g.value.split('-').count(), 4);
    assert_eq!(st.pm_generate(serde_json::from_value(json!({ "kind": "random", "length": 2 })).unwrap()).unwrap_err().code, "invalid_options");

    // imports
    use nicevault_app::passwords::ImportKind;
    let job = st.pm_import(&id, ImportKind::Bitwarden, Some(fixture("bitwarden.json").to_string_lossy().into()), None, None, None).unwrap();
    let done = rec.job_done(&job);
    assert_eq!(done["error"], Value::Null, "{done}");
    assert_eq!(done["imported"], 4);
    let last = rec.progress(&job).last().cloned().unwrap();
    assert_eq!((last["done_bytes"].as_u64(), last["total_bytes"].as_u64()), (Some(4), Some(4)));

    let job = st.pm_import(&id, ImportKind::Csv, None, Some("name,url,username,password\nForum,https://f.example,me,pw-csv\n".into()), None, None).unwrap();
    assert_eq!(rec.job_done(&job)["imported"], 1);

    use base64::Engine;
    let kdbx = base64::engine::general_purpose::STANDARD.encode(std::fs::read(fixture("kdbx31_demopass.kdbx")).unwrap());
    let job = st.pm_import(&id, ImportKind::Keepass, None, Some(kdbx.clone()), Some("sbagliata".to_owned().into()), None).unwrap();
    assert_eq!(rec.job_done(&job)["error"]["code"], "wrong_password");
    let job = st.pm_import(&id, ImportKind::Keepass, None, Some(kdbx), Some("demopass".to_owned().into()), None).unwrap();
    let done = rec.job_done(&job);
    assert_eq!(done["error"], Value::Null, "{done}");
    let kp = done["imported"].as_u64().unwrap();
    assert!(kp >= 5);
    let job = st.pm_import(&id, ImportKind::Bitwarden, None, Some("{\"encrypted\":true}".into()), None, None).unwrap();
    assert_eq!(rec.job_done(&job)["error"]["code"], "unsupported");
    assert_eq!(st.pm_import(&id, ImportKind::Csv, None, None, None, None).unwrap_err().code, "invalid_import");

    let total = 1 + 4 + 1 + kp as usize;
    assert_eq!(st.pm_list(&id).unwrap().len(), total);
    assert!(st.pm_list(&id).unwrap().iter().any(|e| e.folder == "General/Subgroup"));

    // lock: everything refused; reopen: everything back
    st.vault_lock(&id, "user").unwrap();
    assert_eq!(st.pm_list(&id).unwrap_err().code, "locked");
    assert_eq!(st.pm_reveal(&id, &eid, "password").unwrap_err().code, "locked");
    let id = st.vault_open(vol.to_str().unwrap(), pw("segreto")).unwrap().id;
    assert_eq!(st.pm_list(&id).unwrap().len(), total);
    assert_eq!(&*st.pm_reveal(&id, &eid, "password").unwrap(), "Nuova-456");
    st.pm_delete(&id, &eid).unwrap();
    assert_eq!(st.pm_get(&id, &eid).unwrap_err().code, "not_found");
    st.vault_lock(&id, "user").unwrap();

    // nothing readable in the raw file
    let raw = std::fs::read(&vol).unwrap();
    for needle in ["Nuova-456", "anna@example.com", "bw-current-pass-1", "pw-csv", "Sample Entry"] {
        assert!(!raw.windows(needle.len()).any(|w| w == needle.as_bytes()), "{needle} in clear");
    }
}
