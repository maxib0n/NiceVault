use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use std::path::Path;
use tauri::http::{Request, StatusCode, header};

fn get(st: &AppState, uri: &str) -> tauri::http::Response<Vec<u8>> {
    nicevault_app::preview::handle(st, &Request::builder().uri(uri).body(Vec::new()).unwrap())
}

#[test]
fn thumbnails_at_the_size_asked() {
    let t = tempfile::tempdir().unwrap();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(NoEvents));
    let info = st.vault_create(t.path().join("t.nvault").to_str().unwrap(), SecretString::from("pw".to_owned()), Some("fast")).unwrap();
    let v = st.vault(&info.id).unwrap();
    let put = |name: &str, bytes: &[u8]| {
        let n = v.create(ROOT, name, Kind::File).unwrap();
        v.write(n.id, 0, bytes).unwrap();
        n.id
    };
    let photo = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/public/mock/photo-03.jpg")).unwrap();
    let p = put("ritratto.jpg", &photo);
    let txt = put("nota.txt", b"ciao");
    v.flush().unwrap();
    drop(v);

    let r = get(&st, &format!("nvault://{}/{p}?thumb=200", info.id));
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()[header::CONTENT_TYPE], "image/jpeg");
    let img = image::load_from_memory(r.body()).unwrap();
    assert_eq!((img.width(), img.height()), (171, 256), "bucket 256, portrait kept");
    assert!(r.body().len() < photo.len() / 4);

    let again = get(&st, &format!("nvault://{}/{p}?thumb=256", info.id));
    assert_eq!(again.body(), r.body());
    let big = image::load_from_memory(get(&st, &format!("nvault://{}/{p}?thumb=700", info.id)).body()).unwrap();
    assert_eq!(big.height(), 768);
    assert_eq!(image::load_from_memory(get(&st, &format!("nvault://{}/{p}?thumb=4000", info.id)).body()).unwrap().height(), 1024);
    assert_eq!(get(&st, &format!("nvault://{}/{p}", info.id)).body(), &photo);
    assert_eq!(get(&st, &format!("nvault://{}/{txt}?thumb=128", info.id)).body(), b"ciao");

    st.vault_lock(&info.id, "user").unwrap();
    assert_eq!(get(&st, &format!("nvault://{}/{p}?thumb=256", info.id)).status(), StatusCode::NOT_FOUND);
}
