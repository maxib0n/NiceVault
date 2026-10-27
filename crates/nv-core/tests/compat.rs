use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nv_core::passwords::totp::Totp;
use nv_core::passwords::{EntryPatch, Secret};
use nv_core::{Appearance, Identity, KdfPreset, LockedEmblem, OpenError, Vault};
use nv_fs::{Error, Fs, Kind, NodeId, ROOT};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn pat(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

fn sets() -> Vec<(PathBuf, Value)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compat");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no fixture sets in {}", root.display());
    dirs.into_iter()
        .map(|d| {
            let m: Value = serde_json::from_slice(&std::fs::read(d.join("manifest.json")).unwrap()).unwrap();
            assert_eq!(m["manifest_version"], 1, "{}: unknown manifest version", d.display());
            (d, m)
        })
        .collect()
}

fn sets_before_metadata_and_search() -> Vec<(PathBuf, Value)> {
    sets().into_iter().filter(|(_, m)| m["generated_on"].as_str().is_some_and(|d| d < "2026-10-25")).collect()
}

fn contained(want: &Value, got: &Value, at: &str) {
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => {
            for (k, wv) in w {
                let gv = g.get(k).unwrap_or_else(|| panic!("{at}.{k}: missing (have {got})"));
                contained(wv, gv, &format!("{at}.{k}"));
            }
        }
        (Value::Array(w), Value::Array(g)) => {
            assert_eq!(w.len(), g.len(), "{at}: length differs\nwant {want}\ngot  {got}");
            for (i, (wv, gv)) in w.iter().zip(g).enumerate() {
                contained(wv, gv, &format!("{at}[{i}]"));
            }
        }
        _ => assert_eq!(want, got, "{at}"),
    }
}

fn read_all(v: &Vault, id: NodeId) -> Vec<u8> {
    let size = v.getattr(id).unwrap().size as usize;
    let mut out = vec![0u8; size];
    let mut off = 0;
    while off < size {
        let n = v.read(id, off as u64, &mut out[off..(off + 77_777).min(size)]).unwrap();
        assert!(n > 0);
        off += n;
    }
    out
}

fn walk(v: &Vault, dir: NodeId, prefix: &str, out: &mut BTreeMap<String, (nv_fs::Node, Option<Vec<u8>>)>) {
    for n in v.readdir(dir).unwrap() {
        let path = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}/{}", n.name) };
        if n.kind == Kind::Dir {
            walk(v, n.id, &path, out);
            out.insert(path, (n, None));
        } else {
            let data = read_all(v, n.id);
            out.insert(path, (n, Some(data)));
        }
    }
}

fn resolve(v: &Vault, path: &str) -> nv_fs::Result<nv_fs::Node> {
    let mut n = v.getattr(ROOT)?;
    for part in path.split('/') {
        n = v.lookup(n.id, part)?;
    }
    Ok(n)
}

fn appearance_json(a: Option<&Appearance>) -> Value {
    let Some(a) = a else { return Value::Null };
    let emblem = match &a.emblem {
        None => Value::Null,
        Some(LockedEmblem::Initial(s)) => json!({"kind": "initial", "text": s}),
        Some(LockedEmblem::Icon(s)) => json!({"kind": "icon", "id": s}),
        Some(LockedEmblem::Image(b)) => json!({"kind": "image", "base64": B64.encode(b), "sha256": sha(b)}),
    };
    json!({"accent": a.accent, "emblem": emblem})
}

fn copy_of(fixture: &Path, tmp: &Path, tag: &str) -> PathBuf {
    let p = tmp.join(format!("{tag}-{}", fixture.file_name().unwrap().to_string_lossy()));
    std::fs::copy(fixture, &p).unwrap();
    p
}

fn verify_core(v: &Vault, vol: &Value, after_write: bool) {
    let name = vol["file"].as_str().unwrap();
    let core = &vol["core"];
    v.check_integrity().unwrap();

    let mut all = BTreeMap::new();
    walk(v, ROOT, "", &mut all);
    let want = core["nodes"].as_array().unwrap();
    if !after_write {
        let want_paths: Vec<&str> = want.iter().map(|n| n["path"].as_str().unwrap()).collect();
        let got_paths: Vec<&str> = all.keys().map(String::as_str).collect();
        assert_eq!(got_paths, want_paths, "{name}: node set");
        assert_eq!(v.getattr(ROOT).unwrap().mtime, core["root_mtime"].as_i64().unwrap(), "{name}: root mtime");
    }
    for w in want {
        let p = w["path"].as_str().unwrap();
        let (n, data) = all.get(p).unwrap_or_else(|| panic!("{name}: {p} missing"));
        let mut got = json!({"path": p, "id": n.id, "kind": n.kind, "size": n.size, "mtime": n.mtime});
        if let Some(d) = data {
            assert_eq!(d.len() as u64, n.size, "{name}: {p} size vs content");
            got["sha256"] = json!(sha(d));
        }
        contained(w, &got, &format!("{name}:{p}"));
    }
    for p in core["absent"].as_array().unwrap() {
        let p = p.as_str().unwrap();
        assert!(matches!(resolve(v, p), Err(Error::NotFound)), "{name}: {p} should not exist");
    }

    for (k, w) in core["meta"].as_object().unwrap() {
        let got = v.meta_get(k).unwrap();
        match w {
            Value::Null => assert_eq!(got, None, "{name}: meta {k}"),
            w => assert_eq!(B64.encode(got.unwrap_or_else(|| panic!("{name}: meta {k} missing"))), w["base64"].as_str().unwrap(), "{name}: meta {k}"),
        }
    }

    let slots = serde_json::to_value(v.slots()).unwrap();
    let ws = core["slots"].as_array().unwrap();
    let gs = slots.as_array().unwrap();
    if !after_write {
        assert_eq!(ws.len(), gs.len(), "{name}: slot count {slots}");
    }
    for w in ws {
        let g = gs.iter().find(|g| g["index"] == w["index"]).unwrap_or_else(|| panic!("{name}: slot {} missing", w["index"]));
        let mut w = w.clone();
        w.as_object_mut().unwrap().remove("in_use");
        contained(&w, g, &format!("{name}:slot"));
    }
    if !after_write {
        contained(&core["appearance"], &appearance_json(v.appearance().as_ref()), &format!("{name}:appearance"));
    }

    let list = serde_json::to_value(v.pm_list().unwrap()).unwrap();
    let list = list.as_array().unwrap();
    let wpm = core["pm"].as_array().unwrap();
    if !after_write {
        assert_eq!(list.len(), wpm.len(), "{name}: pm entry count");
    }
    for e in wpm {
        let id = e["summary"]["id"].as_str().unwrap();
        let at = format!("{name}:pm:{}", e["summary"]["title"]);
        let g = list.iter().find(|g| g["id"] == id).unwrap_or_else(|| panic!("{at}: missing from pm_list"));
        contained(&e["summary"], g, &at);
        contained(&e["detail"], &serde_json::to_value(v.pm_get(id).unwrap()).unwrap(), &at);
        contained(&e["history"], &serde_json::to_value(v.pm_history(id).unwrap()).unwrap(), &at);
        let s = &e["secrets"];
        let reveal = |w| v.pm_reveal(id, w).ok().map(|x| x.to_string());
        assert_eq!(json!(reveal(Secret::Username)), s["username"], "{at}: username");
        assert_eq!(json!(reveal(Secret::Password)), s["password"], "{at}: password");
        assert_eq!(json!(reveal(Secret::Notes)), s["notes"], "{at}: notes");
        assert_eq!(json!(reveal(Secret::Totp)), s["totp"], "{at}: totp");
        for (i, f) in s["fields"].as_array().unwrap().iter().enumerate() {
            assert_eq!(&json!(reveal(Secret::Field(i))), f, "{at}: field {i}");
        }
        for (i, h) in s["history"].as_array().unwrap().iter().enumerate() {
            assert_eq!(&json!(reveal(Secret::History(i))), h, "{at}: history {i}");
        }
        if let Some(c) = s["totp_code_at"].as_object() {
            let uri = reveal(Secret::Totp).unwrap();
            let t = c["t"].as_u64().unwrap();
            assert_eq!(Totp::parse(&uri).unwrap().code_at(t), c["code"].as_str().unwrap(), "{at}: TOTP code");
        }
    }
}

fn check_volume(dir: &Path, vol: &Value) {
    let name = vol["file"].as_str().unwrap();
    let fixture = dir.join(name);
    let pristine = std::fs::read(&fixture).unwrap();
    assert_eq!(pristine.len() as u64, vol["size"].as_u64().unwrap(), "{name}: fixture file size changed");
    assert_eq!(sha(&pristine), vol["sha256"].as_str().unwrap(), "{name}: fixture file was modified (never edit fixtures)");
    let tmp = tempfile::tempdir().unwrap();
    let passwords: Vec<(usize, String)> = vol["passwords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["slot"].as_u64().unwrap() as usize, p["password"].as_str().unwrap().to_owned()))
        .collect();
    let (main_slot, main_pw) = passwords[0].clone();

    let peek = nv_core::peek(&fixture).unwrap();
    let got = json!({
        "appearance": appearance_json(peek.appearance.as_ref()),
        "password_kdf": peek.password_kdf,
        "recipient_slots": peek.recipient_slots,
    });
    contained(&vol["peek"], &got, &format!("{name}:peek"));

    for (slot, p) in &passwords {
        let v = Vault::open(&copy_of(&fixture, tmp.path(), &format!("pw{slot}")), &pw(p)).unwrap_or_else(|e| panic!("{name}: slot {slot}: {e}"));
        assert_eq!(v.unlocked_slot(), Some(*slot), "{name}");
    }
    assert_eq!(Vault::open(&copy_of(&fixture, tmp.path(), "wrong"), &pw("not the password")).err(), Some(OpenError::WrongPassword));
    let identity = vol["identity"].as_object().map(|i| {
        let ident = Identity::from_file(&dir.join(i["file"].as_str().unwrap()), &pw(i["password"].as_str().unwrap())).unwrap();
        let v = Vault::open_with_identity(&copy_of(&fixture, tmp.path(), "ident"), &ident).unwrap();
        assert_eq!(v.unlocked_slot(), Some(i["slot"].as_u64().unwrap() as usize), "{name}: identity slot");
        ident
    });

    let work = copy_of(&fixture, tmp.path(), "work");
    {
        let v = Vault::open(&work, &pw(&main_pw)).unwrap();
        assert_eq!(v.unlocked_slot(), Some(main_slot));
        assert!(!v.appearance_tampered(), "{name}");
        verify_core(&v, vol, false);
        v.close().unwrap();
    }

    if let Some(d) = vol["duress"].as_object() {
        let c = copy_of(&fixture, tmp.path(), "duress");
        assert_eq!(Vault::open(&c, &pw(d["password"].as_str().unwrap())).err(), Some(OpenError::WrongPassword));
        assert_eq!(Vault::open(&c, &pw(&main_pw)).err(), Some(OpenError::WrongPassword), "{name}: duress did not destroy the keys");
        if let Some(ident) = &identity {
            assert_eq!(Vault::open_with_identity(&c, ident).err(), Some(OpenError::WrongPassword));
        }
    }

    let new_data = pat(77, 300_001);
    let new_pw = "added by a later build";
    {
        let v = Vault::open(&work, &pw(&main_pw)).unwrap();
        let d = v.create(ROOT, "compat-nuovo", Kind::Dir).unwrap();
        let f = v.create(d.id, "nuovo ✓.bin", Kind::File).unwrap();
        v.write(f.id, 0, &new_data).unwrap();
        v.flush().unwrap();
        v.meta_set("compat.test", Some(b"later build")).unwrap();
        let e = v.pm_create(EntryPatch { title: Some("Aggiunta dopo".into()), password: Some(Zeroizing::new("nuova".into())), ..Default::default() }).unwrap();
        assert_eq!(v.pm_get(&e.summary.id).unwrap().summary.title, "Aggiunta dopo");
        let a = v.appearance();
        v.set_appearance(Some(&Appearance { accent: Some("ottanio".into()), emblem: Some(LockedEmblem::Icon("star".into())) })).unwrap();
        v.set_appearance(a.as_ref()).unwrap();
        v.add_password(&pw(new_pw), KdfPreset::Fast, Some("nuova")).unwrap();
        v.close().unwrap();
    }
    for p in [main_pw.as_str(), new_pw] {
        let v = Vault::open(&work, &pw(p)).unwrap_or_else(|e| panic!("{name}: reopen with {p:?}: {e}"));
        verify_core(&v, vol, true);
        let n = resolve(&v, "compat-nuovo/nuovo ✓.bin").unwrap();
        assert_eq!(read_all(&v, n.id), new_data, "{name}: new file");
        assert_eq!(v.meta_get("compat.test").unwrap().as_deref(), Some(&b"later build"[..]));
        assert!(v.pm_list().unwrap().iter().any(|e| e.title == "Aggiunta dopo"));
        assert!(v.slots().iter().any(|s| s.label.as_deref() == Some("nuova")));
        contained(&vol["core"]["appearance"], &appearance_json(v.appearance().as_ref()), &format!("{name}:appearance after write"));
        v.close().unwrap();
    }

    assert_eq!(sha(&std::fs::read(&fixture).unwrap()), vol["sha256"].as_str().unwrap(), "{name}: fixture modified by the test");
    for suffix in ["-wal", "-shm"] {
        assert!(!dir.join(format!("{name}{suffix}")).exists(), "{name}: side file next to the fixture");
    }
}

#[test]
fn every_fixture_volume_still_opens_and_reads_back() {
    for (dir, m) in sets() {
        let vols = m["volumes"].as_array().unwrap();
        assert!(!vols.is_empty());
        for vol in vols {
            check_volume(&dir, vol);
        }
    }
}

#[test]
fn fixture_sets_are_complete() {
    for (dir, m) in sets() {
        let mut listed: Vec<String> = vec!["manifest.json".into(), "README.md".into()];
        for v in m["volumes"].as_array().unwrap() {
            listed.push(v["file"].as_str().unwrap().into());
            if let Some(f) = v["identity"]["file"].as_str() {
                listed.push(f.into());
            }
        }
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        listed.sort();
        on_disk.sort();
        assert_eq!(on_disk, listed, "{}", dir.display());
    }
}

#[test]
fn fixtures_gain_node_metadata_additively() {
    for (dir, m) in sets_before_metadata_and_search() {
        for vol in m["volumes"].as_array().unwrap() {
            let name = vol["file"].as_str().unwrap();
            let p = vol["passwords"][0]["password"].as_str().unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let path = copy_of(&dir.join(name), tmp.path(), "props");
            let v = Vault::open(&path, &pw(p)).unwrap();
            assert_eq!(v.meta_get(nv_core::FILES_SCHEMA_KEY).unwrap().as_deref(), Some(&b"1"[..]), "{name}: migrated");
            let mut nodes = BTreeMap::new();
            walk(&v, ROOT, "", &mut nodes);
            for (path, (n, _)) in &nodes {
                assert_eq!(v.node_meta(n.id).unwrap(), nv_core::NodeMeta::default(), "{name}:{path}: an old node is unknown");
            }
            assert_eq!(v.node_meta(ROOT).unwrap().created, None, "{name}: the old root is not dated");
            let f = v.create(ROOT, "props-new.txt", Kind::File).unwrap();
            v.write(f.id, 0, b"new").unwrap();
            v.set_import_meta(f.id, Some(7), 8, "/host/props-new.txt").unwrap();
            v.flush().unwrap();
            let meta = v.node_meta(f.id).unwrap();
            assert_eq!((meta.created, meta.imported, meta.origin.as_deref()), (Some(7), Some(8), Some("/host/props-new.txt")));
            if let Some((_, (n, _))) = nodes.iter().find(|(_, (n, _))| n.kind == Kind::File) {
                v.remove(n.id).unwrap();
            }
            v.flush().unwrap();
            v.close().unwrap();
            let v = Vault::open(&path, &pw(p)).unwrap();
            assert_eq!(v.node_meta(f.id).unwrap().origin.as_deref(), Some("/host/props-new.txt"), "{name}: persisted");
            v.check_integrity().unwrap();
            v.close().unwrap();
        }
    }
}

#[test]
fn fixtures_gain_search_and_tags_additively() {
    use nv_core::{Filters, SearchQuery};
    for (dir, m) in sets_before_metadata_and_search() {
        for vol in m["volumes"].as_array().unwrap() {
            let name = vol["file"].as_str().unwrap();
            let p = vol["passwords"][0]["password"].as_str().unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let path = copy_of(&dir.join(name), tmp.path(), "search");
            let v = Vault::open(&path, &pw(p)).unwrap();
            assert_eq!(v.meta_get(nv_core::SEARCH_SCHEMA_KEY).unwrap().as_deref(), Some(&b"1"[..]), "{name}: migrated");
            let mut nodes = BTreeMap::new();
            walk(&v, ROOT, "", &mut nodes);
            let all = |text: &str| v.search(&SearchQuery { text: text.into(), limit: 1000, ..Default::default() }).unwrap();
            let page = all("");
            assert_eq!(page.total, nodes.len(), "{name}: every old node is listed");
            assert_eq!(page.pending, nodes.len() as u64, "{name}: no name stored yet");
            for (path, (n, _)) in &nodes {
                let hits = all(&format!("\"{}\"", nv_core::fold(&n.name)));
                let hit = hits.hits.iter().find(|h| h.node.id == n.id).unwrap_or_else(|| panic!("{name}:{path}: not found by its name"));
                let parent = path.rsplit_once('/').map_or("", |(d, _)| d);
                assert_eq!(hit.path, format!("/{parent}"), "{name}:{path}");
                assert_eq!(hit.node, *n, "{name}:{path}");
            }
            let mut left = u64::MAX;
            while left > 0 {
                left = v.search_index_step(3).unwrap();
            }
            assert_eq!(all("").pending, 0, "{name}: filled");
            let tag = v.tag_create("Vecchio", "ardesia").unwrap();
            let old: Vec<_> = nodes.values().map(|(n, _)| n.id).collect();
            assert_eq!(v.tags_set(&[tag.id], &old, true).unwrap().len(), old.len());
            let tagged = v.search(&SearchQuery { filters: Filters { tags: vec![tag.id], ..Default::default() }, limit: 1000, ..Default::default() }).unwrap();
            assert_eq!(tagged.total, old.len());
            if let Some((_, (n, _))) = nodes.iter().find(|(_, (n, _))| n.kind == Kind::File) {
                v.rename(n.id, ROOT, "rinominato-dopo.bin").unwrap();
                assert_eq!(all("rinominato-dopo").hits.len(), 1);
                v.remove(n.id).unwrap();
                assert!(all("rinominato-dopo").hits.is_empty());
            }
            v.flush().unwrap();
            v.close().unwrap();
            let v = Vault::open(&path, &pw(p)).unwrap();
            let left = v.search(&SearchQuery { limit: 1, ..Default::default() }).unwrap().pending;
            assert!(left <= 1, "{name}: stored names survive (only the rename may be queued), {left}");
            assert!(v.tags().unwrap()[0].count >= old.len() as u64 - 1);
            v.check_integrity().unwrap();
            v.close().unwrap();
        }
    }
}

#[test]
fn fixtures_upgrade_to_single_file() {
    for (dir, m) in sets() {
        for vol in m["volumes"].as_array().unwrap() {
            let name = vol["file"].as_str().unwrap();
            let main_pw = vol["passwords"][0]["password"].as_str().unwrap();
            let tmp = tempfile::tempdir().unwrap();
            let work = copy_of(&dir.join(name), tmp.path(), "single");
            let v = Vault::open(&work, &pw(main_pw)).unwrap();
            let was = v.is_single_file();
            assert_eq!(v.upgrade_format().unwrap(), !was, "{name}");
            assert!(v.is_single_file());
            verify_core(&v, vol, false);
            v.close().unwrap();
            let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().flatten().map(|e| e.file_name()).collect();
            assert_eq!(left.len(), 1, "{name}: {left:?}");
            let v = Vault::open(&work, &pw(main_pw)).unwrap();
            verify_core(&v, vol, false);
            let f = v.create(ROOT, "dopo l'aggiornamento", Kind::File).unwrap();
            v.write(f.id, 0, &pat(5, 700_000)).unwrap();
            v.close().unwrap();
            let v = Vault::open(&work, &pw(main_pw)).unwrap();
            verify_core(&v, vol, true);
            assert_eq!(read_all(&v, resolve(&v, "dopo l'aggiornamento").unwrap().id), pat(5, 700_000));
            v.close().unwrap();
            assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1, "{name}: side file after writes");
        }
    }
}
