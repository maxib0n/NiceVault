use nicevault_app::drives::{Drive, Drives, Source};
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events, NoEvents};
use secrecy::SecretString;
use serde_json::Value;
use std::path::Path;
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
    fn count(&self, event: &str) -> usize {
        self.0.lock().unwrap().iter().filter(|(e, _)| e == event).count()
    }
    fn wait_count(&self, event: &str, n: usize) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().filter(|(e, _)| e == event).nth(n - 1) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "no {event} #{n}; got {:?}", self.0.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn write_drives(file: &Path, drives: &[Drive]) {
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(drives).unwrap()).unwrap();
    std::fs::rename(tmp, file).unwrap();
}

fn stick(mount: &Path, free: u64) -> Drive {
    Drive {
        id: "/dev/fake1".into(),
        label: "FAKE STICK".into(),
        mount_path: mount.to_string_lossy().into_owned(),
        fs_type: Some("exfat".into()),
        total_bytes: 32 << 30,
        free_bytes: free,
        removable: true,
        ..Default::default()
    }
}

#[test]
fn plug_and_pull_a_fake_drive() {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("drives.json");
    let mount = t.path().join("media").join("FAKE STICK");
    std::fs::create_dir_all(&mount).unwrap();
    let vol = mount.join("Foto.nvault");
    unsafe { std::env::set_var("NICEVAULT_FAKE_DRIVES", &file) };

    {
        let maker = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(NoEvents));
        let info = maker.vault_create(vol.to_str().unwrap(), SecretString::from("pw".to_owned()), Some("fast")).unwrap();
        maker.vault_lock(&info.id, "user").unwrap();
    }

    let rec = Rec::default();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: None, config_dir: None }), Box::new(rec.clone()));
    let drives = Drives::new(Source::detect());
    assert!(drives.list().is_empty(), "no fake file yet: no drives");
    drives.spawn(Arc::downgrade(&st));
    assert!(st.recent_list().is_empty());

    write_drives(&file, &[stick(&mount, 20 << 30)]);
    let ev = rec.wait_count("drives-changed", 1);
    assert_eq!(ev["drives"][0]["label"], "FAKE STICK");
    assert_eq!(ev["drives"][0]["folder"], mount.to_string_lossy().as_ref(), "a picked drive puts the volume at its root");
    let r = st.recent_list();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].name.as_str(), r[0].source, r[0].exists), ("Foto", "removable", true));

    write_drives(&file, &[stick(&mount, 19 << 30)]);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(rec.count("drives-changed"), 1);
    assert_eq!(drives.list()[0].free_bytes, 19 << 30, "drives_list is fresh");

    let info = st.vault_open(vol.to_str().unwrap(), SecretString::from("pw".to_owned())).unwrap();
    std::fs::rename(&mount, t.path().join("gone")).unwrap();
    write_drives(&file, &[]);
    let ev = rec.wait_count("drives-changed", 2);
    assert_eq!(ev["drives"], serde_json::json!([]));
    let locked = rec.wait_count("vault-locked", 1);
    assert_eq!((locked["vault"].as_str(), locked["reason"].as_str()), (Some(info.id.as_str()), Some("removed")));
    let r = st.recent_list();
    assert_eq!(r.len(), 1, "opened this session: still listed");
    assert_eq!((r[0].source, r[0].exists), ("session", false));
}
