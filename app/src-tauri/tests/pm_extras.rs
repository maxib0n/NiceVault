//! Password manager additions through AppState (what the commands call): folder moves, the
//! import mapping and the repair, site icons (setting, background refresh against a local HTTP
//! site, the nvault:// URL that serves them).

use nicevault_app::pm_icons::{self, Fetcher};
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_core::passwords::EntryPatch;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::io::Cursor;
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
    fn wait(&self, event: &str, pred: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().find(|(e, v)| e == event && pred(v)) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "no {event}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn patch(v: Value) -> EntryPatch {
    serde_json::from_value(v).unwrap()
}

fn app(tmp: &std::path::Path, rec: &Rec) -> Arc<AppState> {
    let cfg = tmp.join("cfg");
    AppState::new(Store::new(Locations { exe_dir: Some(tmp.to_path_buf()), removable: Some(vec![]), config_dir: Some(cfg) }), Box::new(rec.clone()))
}

#[test]
fn folders_and_import_mapping_through_the_app() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = Rec::default();
    let st = app(tmp.path(), &rec);
    let id = st.vault_create(tmp.path().join("F.nvault").to_str().unwrap(), SecretString::from("p".to_owned()), Some("fast")).unwrap().id;
    let a = st.pm_create(&id, patch(json!({ "title": "A", "folder": "Lavoro/Clienti" }))).unwrap().summary.id;
    let b = st.pm_create(&id, patch(json!({ "title": "B" }))).unwrap().summary.id;
    assert_eq!(st.pm_move(&id, &[a.clone(), b.clone()], "Casa").unwrap(), 2);
    assert_eq!(st.pm_folder_rename(&id, "Casa", "Casa nuova").unwrap(), 2);
    assert_eq!(st.pm_folder_rename(&id, "Assente", "X").unwrap_err().code, "not_found");
    assert_eq!(st.pm_folder_delete(&id, "Casa nuova").unwrap(), 2);
    assert_eq!(st.pm_get(&id, &a).unwrap().summary.folder, "");

    // an export, as CSV data
    let csv = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/nv-core/tests/fixtures/autofill.csv")).unwrap();
    let job = st.pm_import(&id, serde_json::from_value(json!("csv")).unwrap(), None, Some(csv), None, None).unwrap();
    let done = rec.wait("job-done", |v| v["job"] == job);
    assert_eq!(done["imported"], 2, "{done}");
    let list = st.pm_list(&id).unwrap();
    let ms = list.iter().find(|e| e.title == "login.microsoftonline.com").unwrap();
    assert_eq!(ms.urls, ["https://login.microsoftonline.com/", "https://account.example.com/"]);
    assert_eq!((ms.created, ms.updated), (1_774_863_307_000, 1_790_593_026_000));
    let d = serde_json::to_value(st.pm_get(&id, &ms.id).unwrap()).unwrap();
    assert_eq!(d["fields"], json!([]));
    assert_eq!(st.pm_repair(&id).unwrap(), 0, "nothing raw left to repair");
}

fn png(w: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, w, image::Rgba([10, 120, 200, 255]));
    let mut c = Cursor::new(Vec::new());
    img.write_to(&mut c, image::ImageFormat::Png).unwrap();
    c.into_inner()
}

#[test]
fn site_icons_refresh_store_and_serve() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = Rec::default();
    let st = app(tmp.path(), &rec);
    assert!(st.app_settings_get().site_icons, "on by default");
    let id = st.vault_create(tmp.path().join("I.nvault").to_str().unwrap(), SecretString::from("p".to_owned()), Some("fast")).unwrap().id;
    let e = st.pm_create(&id, patch(json!({ "title": "Sito", "urls": ["https://shop.example.com/login"] }))).unwrap().summary.id;
    st.pm_create(&id, patch(json!({ "title": "Router", "urls": ["http://192.168.1.1"] }))).unwrap();

    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", server.server_addr().to_ip().unwrap());
    let hits = Arc::new(Mutex::new(0usize));
    let h2 = hits.clone();
    let icon = png(96);
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            *h2.lock().unwrap() += 1;
            let _ = match req.url() {
                "/" => req.respond(
                    tiny_http::Response::from_data(b"<head><link rel=icon sizes=96x96 href=/i.png></head>".to_vec())
                        .with_header("Content-Type: text/html".parse::<tiny_http::Header>().unwrap()),
                ),
                "/i.png" => req.respond(tiny_http::Response::from_data(icon.clone())),
                _ => req.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    let fetcher = Arc::new(Fetcher::for_test(&origin));

    // off: nothing is fetched, and the choice is kept in settings.json
    st.app_settings_set(serde_json::from_value(json!({ "site_icons": false })).unwrap()).unwrap();
    assert_eq!(st.pm_icons_refresh_with(&id, fetcher.clone()).unwrap(), 0);
    assert!(std::fs::read_to_string(tmp.path().join("cfg/settings.json")).unwrap().contains("\"site_icons\": false"));
    assert!(!app(tmp.path(), &Rec::default()).app_settings_get().site_icons);
    assert_eq!(*hits.lock().unwrap(), 0);

    st.app_settings_set(serde_json::from_value(json!({ "site_icons": true })).unwrap()).unwrap();
    assert_eq!(st.pm_icons_refresh_with(&id, fetcher.clone()).unwrap(), 1, "only the real domain");
    rec.wait("pm-icons", |v| v["vault"] == id.as_str() && v["done"] == 1);
    let s = st.pm_get(&id, &e).unwrap().summary;
    let icon_ref = s.icon.clone().expect("icon stored");
    assert!(icon_ref.starts_with("shop.example.com/"));
    // fresh: not fetched again within the week
    assert_eq!(st.pm_icons_refresh_with(&id, fetcher).unwrap(), 0);

    // served from the volume through nvault://, with the version in the URL
    let req = |u: &str| tauri::http::Request::builder().uri(u).body(Vec::new()).unwrap();
    for url in [format!("nvault://{id}/pm-icon/{icon_ref}"), format!("http://nvault.localhost/{id}/pm-icon/{icon_ref}")] {
        let r = nicevault_app::preview::handle(&st, &req(&url));
        assert_eq!(r.status(), 200, "{url}");
        assert_eq!(r.headers()["content-type"], "image/png");
        let got = image::load_from_memory(r.body()).unwrap();
        assert_eq!(got.width(), pm_icons::ICON_PX);
    }
    assert_eq!(nicevault_app::preview::handle(&st, &req(&format!("nvault://{id}/pm-icon/other.example/1"))).status(), 404);
    st.vault_lock(&id, "user").unwrap();
    assert_eq!(nicevault_app::preview::handle(&st, &req(&format!("nvault://{id}/pm-icon/{icon_ref}"))).status(), 404);
}
