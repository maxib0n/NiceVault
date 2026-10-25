//! Search, filters and tags end to end through the functions the Tauri commands call, with the
//! JSON shapes api.ts relies on: fs_search pages / highlights / paths / filters, the background
//! index job and its events, fs_list with tags and filters, the tags_* commands, their errors
//! and tags-changed events, and everything surviving a lock + reopen.

use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, Events};
use nv_core::{Filters, KindGroup};
use nv_fs::{Fs, Kind, ROOT};
use secrecy::SecretString;
use serde_json::{Value, json};
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
    fn take(&self, event: &str) -> Vec<Value> {
        let mut g = self.0.lock().unwrap();
        let (hit, rest): (Vec<_>, Vec<_>) = g.drain(..).partition(|(e, _)| e == event);
        *g = rest;
        hit.into_iter().map(|(_, v)| v).collect()
    }

    fn wait(&self, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some((_, v)) = self.0.lock().unwrap().iter().find(|(e, v)| e == what && ok(v)) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(30), "no {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn pw() -> SecretString {
    SecretString::from("pw".to_owned())
}

#[test]
fn search_filters_tags_through_the_app() {
    let t = tempfile::tempdir().unwrap();
    let rec = Rec::default();
    let st = AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: None }), Box::new(rec.clone()));
    let vol = t.path().join("s.nvault");
    let id = st.vault_create(vol.to_str().unwrap(), pw(), Some("fast")).unwrap().id;
    let foto = st.fs_mkdir(&id, ROOT, "Foto").unwrap();
    let mare = st.fs_mkdir(&id, foto.id, "Mare d'estate").unwrap();
    let v = st.vault(&id).unwrap();
    let f1 = v.create(mare.id, "Città al MARE.jpg", Kind::File).unwrap();
    v.write(f1.id, 0, &[1; 5000]).unwrap();
    let f2 = v.create(ROOT, "Bolletta.pdf", Kind::File).unwrap();
    v.write(f2.id, 0, &[1; 100]).unwrap();
    v.flush().unwrap();

    // --- fs_search: page shape, highlights in UTF-16, folder path, cursor
    let r = st.fs_search(&id, "mare", None, None, false, None, None).unwrap();
    let j = serde_json::to_value(&r).unwrap();
    assert_eq!(j["total"], 2);
    assert_eq!(j["next_cursor"], Value::Null);
    assert_eq!(j["hits"][0]["node"]["name"], "Mare d'estate");
    assert_eq!(j["hits"][0]["node"]["kind"], "dir");
    assert_eq!(j["hits"][0]["path"], "/Foto");
    assert_eq!(j["hits"][1]["path"], "/Foto/Mare d'estate");
    assert_eq!(j["hits"][1]["highlights"], json!([[9, 13]]));
    assert_eq!(j["hits"][1]["tags"], json!([]));
    let p1 = st.fs_search(&id, "", None, Some("name"), false, Some(2), None).unwrap();
    assert_eq!((p1.total, p1.hits.len(), p1.next_cursor.as_deref()), (4, 2, Some("o2")));
    let p2 = st.fs_search(&id, "", None, Some("name"), false, Some(2), p1.next_cursor.as_deref()).unwrap();
    assert_eq!((p2.hits.len(), p2.next_cursor.clone()), (2, None));
    assert_eq!(st.fs_search(&id, "", None, Some("nope"), false, None, None).unwrap_err().code, "invalid_query");
    assert_eq!(st.fs_search(&id, "", None, None, false, None, Some("x")).unwrap_err().code, "invalid_query");
    // filters arrive as JSON from the UI
    let photos: Filters = serde_json::from_value(json!({ "kinds": ["photos"], "min_size": 1000 })).unwrap();
    assert_eq!(st.fs_search(&id, "", Some(photos), None, false, None, None).unwrap().hits[0].node.id, f1.id);

    // the background job persisted the new names and said so
    let done = rec.wait("search-index", |v| v["done"] == v["total"] && v["total"].as_u64() > Some(0));
    assert_eq!(done["vault"], id.as_str());
    let t0 = Instant::now();
    while st.fs_search(&id, "x", None, None, false, None, None).unwrap().pending > 0 {
        assert!(t0.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }

    // --- tags
    rec.take("tags-changed");
    let viaggi = st.tags_create(&id, "Viaggi", "prussia").unwrap();
    assert_eq!(serde_json::to_value(&viaggi).unwrap(), json!({ "id": viaggi.id, "name": "Viaggi", "color": "prussia", "count": 0 }));
    assert_eq!(rec.take("tags-changed"), vec![json!({ "vault": id, "nodes": [], "dirs": [] })]);
    assert_eq!(st.tags_create(&id, "viaggi", "neutral").unwrap_err().code, "exists");
    assert_eq!(st.tags_create(&id, " ", "neutral").unwrap_err().code, "invalid_name");
    assert_eq!(st.tags_create(&id, "Rosso", "rosso").unwrap_err().code, "invalid_color");
    let casa = st.tags_create(&id, "Casa", "seppia").unwrap();
    rec.take("tags-changed");
    let all = st.tags_set(&id, &[viaggi.id], &[f1.id, mare.id], true).unwrap();
    assert_eq!(all.iter().map(|t| (t.name.as_str(), t.count)).collect::<Vec<_>>(), vec![("Casa", 0), ("Viaggi", 2)]);
    let ev = rec.take("tags-changed");
    assert_eq!(ev, vec![json!({ "vault": id, "nodes": [mare.id, f1.id], "dirs": [foto.id, mare.id] })]);
    st.tags_set(&id, &[casa.id], &[f2.id], true).unwrap();
    assert_eq!(st.tags_set(&id, &[999], &[f2.id], true).unwrap_err().code, "not_found");
    rec.take("tags-changed");
    // nothing changed: no event
    st.tags_set(&id, &[casa.id], &[f2.id], true).unwrap();
    assert!(rec.take("tags-changed").is_empty());

    // fs_list: tags on each node (absent when none), filters
    let l = serde_json::to_value(st.fs_list_tagged(&id, ROOT, None).unwrap()).unwrap();
    assert_eq!(l, json!([
        { "id": f2.id, "parent": ROOT, "name": "Bolletta.pdf", "kind": "file", "size": 100, "mtime": l[0]["mtime"], "tags": [casa.id] },
        { "id": foto.id, "parent": ROOT, "name": "Foto", "kind": "dir", "size": 0, "mtime": l[1]["mtime"] },
    ]));
    let docs = Filters { kinds: vec![KindGroup::Documents], ..Default::default() };
    assert_eq!(st.fs_list_tagged(&id, ROOT, Some(docs)).unwrap().len(), 1);
    let tagged = Filters { tags: vec![viaggi.id], ..Default::default() };
    assert_eq!(st.fs_list_tagged(&id, foto.id, Some(tagged.clone())).unwrap()[0].node.id, mare.id);
    assert!(st.fs_list_tagged(&id, ROOT, Some(tagged.clone())).unwrap().is_empty());
    let r = st.fs_search(&id, "", Some(tagged), None, false, None, None).unwrap();
    assert_eq!(r.hits.iter().map(|h| h.node.id).collect::<Vec<_>>(), vec![mare.id, f1.id]);
    assert_eq!(r.hits[1].tags, vec![viaggi.id]);
    let of = serde_json::to_value(st.tags_of(&id, &[f1.id, f2.id, foto.id]).unwrap()).unwrap();
    assert_eq!(of, json!({ f1.id.to_string(): [viaggi.id], f2.id.to_string(): [casa.id] }));

    let up = st.tags_update(&id, viaggi.id, Some("Vacanze"), Some("bottiglia")).unwrap();
    assert_eq!((up.name.as_str(), up.color.as_str(), up.count), ("Vacanze", "bottiglia", 2));
    st.tags_set(&id, &[viaggi.id], &[mare.id], false).unwrap();
    assert_eq!(rec.take("tags-changed").last().unwrap()["nodes"], json!([mare.id]));
    st.tags_delete(&id, casa.id).unwrap();
    assert_eq!(rec.take("tags-changed"), vec![json!({ "vault": id, "nodes": [f2.id], "dirs": [ROOT] })]);
    assert_eq!(st.tags_delete(&id, casa.id).unwrap_err().code, "not_found");

    // renames and deletes through the app are seen by the next search
    st.fs_rename(&id, f1.id, ROOT, "Lungomare.jpg").unwrap();
    let r = st.fs_search(&id, "lungomare", None, None, false, None, None).unwrap();
    assert_eq!((r.hits[0].path.as_str(), r.hits[0].tags.clone()), ("/", vec![viaggi.id]));
    st.fs_delete(&id, &[foto.id]).unwrap();
    assert_eq!(st.fs_search(&id, "mare", None, None, false, None, None).unwrap().hits.len(), 1);

    // lock: commands answer "locked"; reopen: tags and index are in the volume
    drop(v);
    st.vault_lock(&id, "user").unwrap();
    assert_eq!(st.fs_search(&id, "x", None, None, false, None, None).unwrap_err().code, "locked");
    assert_eq!(st.tags_list(&id).unwrap_err().code, "locked");
    let id = st.vault_open(vol.to_str().unwrap(), pw()).unwrap().id;
    let tags = st.tags_list(&id).unwrap();
    assert_eq!(tags.iter().map(|t| (t.name.as_str(), t.count)).collect::<Vec<_>>(), vec![("Vacanze", 1)]);
    let r = st.fs_search(&id, "LUNGOMARE", None, None, false, None, None).unwrap();
    assert_eq!((r.hits.len(), r.pending), (1, 0));
    assert!(st.vault(&id).unwrap().readdir(ROOT).unwrap().len() == 2);
    st.vault_lock(&id, "user").unwrap();
}
