use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_fs::{Kind, ROOT};
use secrecy::SecretString;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<(String, Value)>>>);

impl Events for Rec {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

impl Rec {
    fn done(&self, event: &str, job: &str) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().find(|(e, v)| e == event && v["job"] == job) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "no {event} for {job}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn mock_asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/public/mock").join(name)
}

fn mtime_ns(p: &Path) -> i64 {
    std::fs::metadata(p).unwrap().modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64
}

#[test]
fn properties_of_imported_files_folders_and_hashes() {
    let t = tempfile::tempdir().unwrap();
    let host = t.path().join("host");
    std::fs::create_dir_all(host.join("Album/Sub")).unwrap();
    for (src, dst) in [("photo-01.jpg", "foto.jpg"), ("card.png", "card.png"), ("clip.webm", "clip.webm"), ("tone.ogg", "tone.ogg"), ("document.pdf", "doc.pdf")] {
        std::fs::copy(mock_asset(src), host.join(dst)).unwrap();
    }
    std::fs::write(host.join("Album/a.txt"), b"uno\n").unwrap();
    std::fs::write(host.join("Album/Sub/b.txt"), b"due due\n").unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    std::fs::File::options().write(true).open(host.join("doc.pdf")).unwrap().set_modified(old).unwrap();

    let rec = Rec::default();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(rec.clone()));
    let vol = t.path().join("p.nvault");
    let info = st.vault_create(vol.to_str().unwrap(), SecretString::from("pw".to_owned()), Some("fast")).unwrap();
    let id = info.id.clone();
    let before = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64;
    let paths: Vec<String> = ["foto.jpg", "card.png", "clip.webm", "tone.ogg", "doc.pdf", "Album"].iter().map(|n| host.join(n).to_string_lossy().into_owned()).collect();
    let job = nicevault_app::jobs::fs_import(&st, &id, ROOT, paths).unwrap();
    let r = rec.done("job-done", &job);
    assert!(r["error"].is_null(), "{r}");

    let node = |name: &str| st.fs_list(&id, ROOT).unwrap().into_iter().find(|n| n.name == name).unwrap();
    let p = st.fs_props(&id, node("foto.jpg").id).unwrap();
    assert_eq!((p.mime.as_deref(), p.width, p.height), (Some("image/jpeg"), Some(960), Some(659)));
    assert_eq!(p.path, "/foto.jpg");
    assert_eq!(p.origin.as_deref(), Some(host.join("foto.jpg").to_str().unwrap()));
    assert!(p.imported.unwrap() >= before, "imported now");
    assert!(p.created.is_some(), "the host file's birth time (or the import time)");
    assert_eq!(p.mtime, mtime_ns(&host.join("foto.jpg")), "original mtime kept");

    let p = st.fs_props(&id, node("card.png").id).unwrap();
    assert_eq!((p.mime.as_deref(), p.width, p.height), (Some("image/png"), Some(428), Some(270)));
    let p = st.fs_props(&id, node("clip.webm").id).unwrap();
    assert_eq!((p.mime.as_deref(), p.duration_ms), (Some("video/webm"), Some(4000)));
    let p = st.fs_props(&id, node("tone.ogg").id).unwrap();
    assert_eq!(p.mime.as_deref(), Some("audio/opus"), "tone.ogg is Opus in an Ogg");
    assert!(p.duration_ms.is_some_and(|d| d.abs_diff(3006) <= 30), "{:?}", p.duration_ms);
    let p = st.fs_props(&id, node("doc.pdf").id).unwrap();
    assert_eq!((p.mime.as_deref(), p.mtime), (Some("application/pdf"), 1_600_000_000_000_000_000));
    assert_eq!(p.sha256, None, "not computed yet");

    let album = node("Album");
    let p = st.fs_props(&id, album.id).unwrap();
    assert_eq!((p.kind, p.children, p.origin.as_deref()), (Kind::Dir, Some(2), Some(host.join("Album").to_str().unwrap())));
    let job = st.fs_props_tree(&id, vec![album.id]).unwrap();
    let d = rec.done("props-done", &job);
    assert_eq!((d["files"].as_u64(), d["dirs"].as_u64(), d["bytes"].as_u64(), d["error"].is_null()), (Some(2), Some(1), Some(12), true));
    let job = st.fs_props_tree(&id, vec![node("foto.jpg").id, node("card.png").id, album.id]).unwrap();
    let d = rec.done("props-done", &job);
    let want = std::fs::metadata(host.join("foto.jpg")).unwrap().len() + std::fs::metadata(host.join("card.png")).unwrap().len() + 12;
    assert_eq!((d["files"].as_u64(), d["dirs"].as_u64(), d["bytes"].as_u64()), (Some(4), Some(1), Some(want)));
    let job = st.fs_props_tree(&id, vec![ROOT]).unwrap();
    assert_eq!(rec.done("props-done", &job)["files"].as_u64(), Some(7));

    let pdf = node("doc.pdf");
    let job = st.fs_hash(&id, pdf.id).unwrap();
    let d = rec.done("props-done", &job);
    let want: String = Sha256::digest(std::fs::read(host.join("doc.pdf")).unwrap()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(d["sha256"].as_str(), Some(want.as_str()));
    assert_eq!(st.fs_props(&id, pdf.id).unwrap().sha256.as_deref(), Some(want.as_str()));
    assert!(st.fs_hash(&id, album.id).is_err(), "a folder has no hash");

    let job = st.fs_hash(&id, node("clip.webm").id).unwrap();
    st.job_cancel(&job);
    let d = rec.done("props-done", &job);
    assert!(d["error"].is_null() || d["error"]["code"] == "cancelled", "{d}");

    st.vault_lock(&id, "user").unwrap();
    let info = st.vault_open(vol.to_str().unwrap(), SecretString::from("pw".to_owned())).unwrap();
    let p = st.fs_props(&info.id, pdf.id).unwrap();
    assert_eq!((p.sha256.as_deref(), p.origin.as_deref()), (Some(want.as_str()), Some(host.join("doc.pdf").to_str().unwrap())));

    let n = st.fs_mkdir(&info.id, ROOT, "Nuova").unwrap();
    let p = st.fs_props(&info.id, n.id).unwrap();
    assert_eq!((p.created.is_some(), p.imported, p.origin), (true, None, None));
    let root = st.fs_props(&info.id, ROOT).unwrap();
    assert_eq!((root.path.as_str(), root.children), ("/", Some(7)));
}
