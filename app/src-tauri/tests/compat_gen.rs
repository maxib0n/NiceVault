use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use nicevault_app::appearance::{SettingsPatch, Visibility};
use nicevault_app::emblem::Emblem;
use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use nv_core::passwords::totp::Totp;
use nv_core::passwords::{EntryPatch, FieldInput, ImportedEntry, Secret};
use nv_core::{Appearance, CHUNK, Identity, KdfPreset, LockedEmblem, Vault};
use nv_fs::{Fs, Kind, NodeId, ROOT};
use secrecy::SecretString;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zeroize::Zeroizing;

const TOTP_T: u64 = 1_700_000_000;
const META_KEYS: [&str; 5] = ["volume_id", "nicevault.settings", "nicevault.emblem", "nicevault.emblem_locked", "nv.pm_schema"];

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}
fn z(s: &str) -> Option<Zeroizing<String>> {
    Some(Zeroizing::new(s.to_owned()))
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

fn png(side: u32, len: usize, fill: &[u8]) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    v.extend_from_slice(&side.to_be_bytes());
    v.extend_from_slice(&side.to_be_bytes());
    while v.len() < len {
        v.extend_from_slice(fill);
    }
    v.truncate(len);
    v
}

fn portable(stick: &Path) -> Arc<AppState> {
    AppState::new(Store::new(Locations { exe_dir: Some(stick.to_path_buf()), removable: Some(vec![]), config_dir: None }), Box::new(NoEvents))
}

fn patch(v: Value) -> SettingsPatch {
    serde_json::from_value(v).unwrap()
}

fn mkdir(v: &Vault, parent: NodeId, name: &str) -> NodeId {
    v.create(parent, name, Kind::Dir).unwrap().id
}

fn put(v: &Vault, parent: NodeId, name: &str, data: &[u8]) -> NodeId {
    let id = v.create(parent, name, Kind::File).unwrap().id;
    for (i, c) in data.chunks(99_991).enumerate() {
        assert_eq!(v.write(id, (i * 99_991) as u64, c).unwrap(), c.len());
    }
    id
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

fn fix_mtimes(v: &Vault, base: i64) {
    let mut all = BTreeMap::new();
    walk(v, ROOT, "", &mut all);
    v.set_mtime(ROOT, base).unwrap();
    for (n, _) in all.values() {
        v.set_mtime(n.id, base + n.id as i64 * 1_000_003_007 + 123).unwrap();
    }
    v.flush().unwrap();
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

struct Spec {
    file: &'static str,
    description: &'static str,
    passwords: Vec<(usize, &'static str)>,
    duress: Option<(usize, &'static str)>,
    identity: Option<(&'static str, &'static str, usize)>,
    contents: BTreeMap<String, Vec<u8>>,
    absent: Vec<&'static str>,
}

const ARCH_PW: &str = "archivio-principale";
const ARCH_PW2: &str = "riserva ✓ üñí 密码";

fn gen_archivio(dir: &Path) -> Spec {
    let path = dir.join("archivio.nvault");
    let st = portable(dir);
    let info = st.vault_create(path.to_str().unwrap(), pw(ARCH_PW), Some("fast")).unwrap();
    let id = info.id.clone();
    let v = st.vault(&id).unwrap();
    let mut c: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    let docs = mkdir(&v, ROOT, "Documenti");
    let foto = mkdir(&v, docs, "Foto vacanze");
    let sotto = mkdir(&v, docs, "Sotto");
    let giu = mkdir(&v, sotto, "Più giù");
    mkdir(&v, ROOT, "Vuota");
    let mut file = |parent, dirpath: &str, name: &str, data: Vec<u8>| {
        let id = put(&v, parent, name, &data);
        c.insert(if dirpath.is_empty() { name.to_owned() } else { format!("{dirpath}/{name}") }, data);
        id
    };
    file(ROOT, "", "vuoto.txt", vec![]);
    file(ROOT, "", "nota.txt", b"Ciao, mondo!\n".to_vec());
    file(ROOT, "", ".nascosto", b"dotfile".to_vec());
    file(docs, "Documenti", "esattamente-un-chunk.bin", pat(1, CHUNK));
    file(foto, "Documenti/Foto vacanze", "grande dispari.bin", pat(2, 2 * CHUNK + 4321));
    file(foto, "Documenti/Foto vacanze", "Città ☕ 日本.md", "# Città ☕ 日本\nàèìòù — «virgolette» 🦊\n".as_bytes().to_vec());
    file(docs, "Documenti", "Cafe\u{301}.txt", b"name in NFD (e + combining acute)".to_vec());
    file(giu, "Documenti/Sotto/Più giù", "profondo.txt", b"in fondo\n".to_vec());
    let mut m = pat(3, 300_000);
    let mid = pat(4, 10_000);
    let mid_id = file(ROOT, "", "modificato.bin", m.clone());
    v.write(mid_id, 100_000, &mid).unwrap();
    v.set_size(mid_id, 299_999).unwrap();
    m[100_000..110_000].copy_from_slice(&mid);
    m.truncate(299_999);
    c.insert("modificato.bin".into(), m);
    let b = put(&v, ROOT, "bozza.txt", b"prima stesura\n");
    v.rename(b, docs, "Rinominato definitivo.txt").unwrap();
    c.insert("Documenti/Rinominato definitivo.txt".into(), b"prima stesura\n".to_vec());
    let e = put(&v, ROOT, "eliminato.txt", b"via");
    v.remove(e).unwrap();
    let cd = mkdir(&v, ROOT, "Cartella eliminata");
    put(&v, cd, "dentro.txt", b"x");
    v.remove_recursive(cd).unwrap();
    v.flush().unwrap();

    st.settings_set(
        patch(json!({"theme": "light", "auto_lock_minutes": 5, "lock_on_sleep": false, "lock_on_screen_lock": true, "lock_scope": "all"})),
        Some(&id),
    )
    .unwrap();
    let full = png(256, 4000, b"NV-COMPAT-FULL-256 ");
    let small = png(128, 1500, b"NV-COMPAT-SMALL-128 ");
    st.vault_appearance_set(
        &id,
        Some("velluto".into()),
        Some(Emblem::Image { data_base64: B64.encode(&full) }),
        Some(Visibility::Visible),
        Some(B64.encode(&small)),
        None,
    )
    .unwrap();

    v.set_slot_label(0, Some("Principale")).unwrap();
    assert_eq!(v.add_password(&pw(ARCH_PW2), KdfPreset::Fast, Some("Riserva — cassaforte 🔐")).unwrap(), 1);
    assert_eq!(v.add_password(&pw("temporanea"), KdfPreset::Fast, Some("da togliere")).unwrap(), 2);
    let ident = Identity::generate();
    ident.to_file_with(&dir.join("archivio.nvkey"), &pw("chiave-identita"), KdfPreset::Fast).unwrap();
    assert_eq!(v.add_recipient(&ident.recipient(), Some("Chiave di casa")).unwrap(), 3);
    assert_eq!(v.add_duress(&pw("archivio-distruggi"), KdfPreset::Fast, Some("Emergenza")).unwrap(), 4);
    v.remove_slot(2).unwrap();

    let e1 = v
        .pm_create(EntryPatch {
            title: Some("Posta Ufficio".into()),
            username: Some("mario.rossi@example.it".into()),
            password: z("Prima#1"),
            urls: Some(vec!["https://mail.example.it".into(), "https://webmail.example.it/login".into()]),
            notes: z("PIN della porta: 1234\nseconda riga ✓"),
            totp: z("otpauth://totp/Example:mario?secret=JBSWY3DPEHPK3PXP&issuer=Example"),
            folder: Some("Lavoro/Posta".into()),
            tags: Some(vec!["lavoro".into(), "email".into()]),
            favorite: Some(true),
            fields: Some(vec![
                FieldInput { name: "Domanda".into(), value: z("Nome del gatto"), hidden: false },
                FieldInput { name: "Risposta".into(), value: z("Fuffi-Segreto-889"), hidden: true },
            ]),
        })
        .unwrap();
    let e1 = e1.summary.id;
    v.pm_update(&e1, EntryPatch { password: z("Seconda#2"), ..Default::default() }).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    v.pm_update(&e1, EntryPatch { password: z("Terza#3"), ..Default::default() }).unwrap();
    v.pm_create(EntryPatch {
        title: Some("Banca Ünïcode".into()),
        username: Some("cliente 0042".into()),
        password: z("pässwörd with spaces and 🦊"),
        totp: z("otpauth://totp/Banca:mario?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Banca&algorithm=SHA256&digits=8&period=60"),
        folder: Some("Finanza".into()),
        ..Default::default()
    })
    .unwrap();
    v.pm_create(EntryPatch { title: Some("Solo nota".into()), notes: z("nessuna password qui"), ..Default::default() }).unwrap();
    v.pm_insert_many(vec![ImportedEntry {
        entry: EntryPatch {
            title: Some("Importata da Bitwarden".into()),
            username: Some("imp".into()),
            password: z("Attuale-Imp"),
            totp: z("JBSWY3DPEHPK3PXP"),
            tags: Some(vec!["importata".into()]),
            ..Default::default()
        },
        history: vec![(Zeroizing::new("Vecchia-1".into()), 1_600_000_000_000), (Zeroizing::new("Vecchia-2".into()), 1_650_000_000_000)],
        created: Some(1_500_000_000_000),
        updated: Some(1_690_000_000_000),
    }])
    .unwrap();
    let gone = v.pm_create(EntryPatch { title: Some("Da eliminare".into()), password: z("x"), ..Default::default() }).unwrap();
    v.pm_delete(&gone.summary.id).unwrap();

    fix_mtimes(&v, 1_600_000_000_000_000_000);
    drop(v);
    st.vault_lock(&id, "user").unwrap();
    Spec {
        file: "archivio.nvault",
        description: "Everything this build writes: files of several sizes (empty, small, exactly one chunk, 3-chunk odd size, \
                      overwritten + truncated), nested folders, unicode / NFD / space / dot names, a renamed+moved file, a deleted \
                      file and folder, fixed mtimes; image emblem visible (256 px inside, 128 px copy in the header), accent velluto, \
                      theme light, auto-lock 5 min / no lock on sleep / scope all; slots password(labelled), password(unicode pw, \
                      labelled), hole, recipient (archivio.nvkey), duress; 4 password-manager entries with TOTP (SHA1/SHA256-8-60), \
                      hidden field, history (2 from updates, 2 imported), one deleted entry.",
        passwords: vec![(0, ARCH_PW), (1, ARCH_PW2)],
        duress: Some((4, "archivio-distruggi")),
        identity: Some(("archivio.nvkey", "chiave-identita", 3)),
        contents: c,
        absent: vec!["bozza.txt", "eliminato.txt", "Cartella eliminata", "Cartella eliminata/dentro.txt"],
    }
}

fn gen_cifrato(dir: &Path) -> Spec {
    let path = dir.join("cifrato.nvault");
    let st = portable(dir);
    let info = st.vault_create(path.to_str().unwrap(), pw("cifrato-pw"), Some("fast")).unwrap();
    let id = info.id.clone();
    let v = st.vault(&id).unwrap();
    let mut c = BTreeMap::new();
    put(&v, ROOT, "segreto.txt", b"solo dentro\n");
    c.insert("segreto.txt".to_owned(), b"solo dentro\n".to_vec());
    let d = mkdir(&v, ROOT, "Dir");
    put(&v, d, "a.bin", &pat(5, 1000));
    c.insert("Dir/a.bin".to_owned(), pat(5, 1000));
    v.flush().unwrap();
    st.settings_set(patch(json!({"theme": "system", "auto_lock_minutes": 0, "lock_on_screen_lock": false})), Some(&id)).unwrap();
    st.vault_appearance_set(&id, Some("prussia".into()), Some(Emblem::Icon { id: "key".into() }), Some(Visibility::Encrypted), None, None).unwrap();
    v.set_slot_label(0, Some("Unica")).unwrap();
    v.pm_create(EntryPatch {
        title: Some("Server".into()),
        username: Some("root".into()),
        password: z("hunter2"),
        totp: z("otpauth://totp/Srv:root?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQGEZA&issuer=Srv&algorithm=SHA512&digits=7&period=45"),
        ..Default::default()
    })
    .unwrap();
    fix_mtimes(&v, 1_700_000_000_000_000_000);
    drop(v);
    st.vault_lock(&id, "user").unwrap();
    Spec {
        file: "cifrato.nvault",
        description: "Appearance ENCRYPTED (no header section): icon emblem 'key' + accent prussia live only inside; theme system, \
                      auto-lock never, no lock on screen lock; one labelled password slot; one PM entry (TOTP SHA512-7-45).",
        passwords: vec![(0, "cifrato-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_bordeaux(dir: &Path) -> Spec {
    let path = dir.join("bordeaux.nvault");
    let st = portable(dir);
    let info = st.vault_create(path.to_str().unwrap(), pw("bordeaux-pw"), Some("fast")).unwrap();
    {
        let v = st.vault(&info.id).unwrap();
        put(&v, ROOT, "vecchio.txt", b"dai tempi del bordeaux\n");
        v.flush().unwrap();
        fix_mtimes(&v, 1_650_000_000_000_000_000);
    }
    st.vault_lock(&info.id, "user").unwrap();
    let v = Vault::open(&path, &pw("bordeaux-pw")).unwrap();
    v.meta_set("nicevault.settings", Some(br#"{"auto_lock_minutes":30,"accent":"bordeaux"}"#)).unwrap();
    v.set_appearance(Some(&Appearance { accent: Some("bordeaux".into()), emblem: Some(LockedEmblem::Initial("B".into())) })).unwrap();
    v.close().unwrap();
    let mut c = BTreeMap::new();
    c.insert("vecchio.txt".to_owned(), b"dai tempi del bordeaux\n".to_vec());
    Spec {
        file: "bordeaux.nvault",
        description: "Simulated pre-velluto volume: settings JSON {auto_lock_minutes:30, accent:bordeaux} with no theme / lock fields, \
                      header section accent 'bordeaux' + Initial B. Must read as velluto, theme dark, other fields defaulted.",
        passwords: vec![(0, "bordeaux-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_nudo(dir: &Path) -> Spec {
    let path = dir.join("nudo.nvault");
    let v = Vault::create(&path, &pw("nudo-pw"), KdfPreset::Strong).unwrap();
    put(&v, ROOT, "solo.txt", b"core only\n");
    v.flush().unwrap();
    fix_mtimes(&v, 1_500_000_000_000_000_000);
    v.close().unwrap();
    let mut c = BTreeMap::new();
    c.insert("solo.txt".to_owned(), b"core only\n".to_vec());
    Spec {
        file: "nudo.nvault",
        description: "nv-core only (no app meta keys, no header appearance section), KdfPreset::STRONG (the real-volume Argon2id \
                      parameters, p = 4), one unlabelled slot, one file.",
        passwords: vec![(0, "nudo-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn record(fixtures: &Path, s: &Spec) -> Value {
    let raw = std::fs::read(fixtures.join(s.file)).unwrap();
    for suffix in ["-wal", "-shm"] {
        assert!(!fixtures.join(format!("{}{suffix}", s.file)).exists(), "{} left a side file", s.file);
    }
    let peek = nv_core::peek(&fixtures.join(s.file)).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let copy = |name: &str| {
        let p = scratch.path().join(format!("{}-{name}", rand_name()));
        std::fs::copy(fixtures.join(s.file), &p).unwrap();
        p
    };

    for (slot, p) in &s.passwords {
        let v = Vault::open(&copy("pw"), &pw(p)).unwrap();
        assert_eq!(v.unlocked_slot(), Some(*slot));
    }
    if let Some((f, p, slot)) = s.identity {
        let ident = Identity::from_file(&fixtures.join(f), &pw(p)).unwrap();
        let v = Vault::open_with_identity(&copy("id"), &ident).unwrap();
        assert_eq!(v.unlocked_slot(), Some(slot));
    }

    let v = Vault::open(&copy("core"), &pw(s.passwords[0].1)).unwrap();
    v.check_integrity().unwrap();
    let mut all = BTreeMap::new();
    walk(&v, ROOT, "", &mut all);
    let files: BTreeMap<&String, &Vec<u8>> = all.iter().filter_map(|(p, (_, d))| d.as_ref().map(|d| (p, d))).collect();
    assert_eq!(files.len(), s.contents.len(), "{}: {:?}", s.file, files.keys());
    for (p, want) in &s.contents {
        assert!(files.get(p).is_some_and(|d| *d == want), "{}: {p} reads back different", s.file);
    }
    let nodes: Vec<Value> = all
        .iter()
        .map(|(p, (n, d))| {
            let mut o = json!({"path": p, "id": n.id, "kind": n.kind, "size": n.size, "mtime": n.mtime});
            if let Some(d) = d {
                o["sha256"] = json!(sha(d));
            }
            o
        })
        .collect();
    let root = v.getattr(ROOT).unwrap();
    let meta: serde_json::Map<String, Value> = META_KEYS
        .iter()
        .map(|k| {
            let val = v.meta_get(k).unwrap().map(|b| {
                let mut o = json!({"base64": B64.encode(&b), "sha256": sha(&b)});
                if let Ok(t) = std::str::from_utf8(&b) {
                    o["utf8"] = json!(t);
                }
                o
            });
            (k.to_string(), val.unwrap_or(Value::Null))
        })
        .collect();
    let pm: Vec<Value> = v
        .pm_list()
        .unwrap()
        .iter()
        .map(|e| {
            let id = &e.id;
            let d = v.pm_get(id).unwrap();
            let reveal = |w| v.pm_reveal(id, w).ok().map(|x| x.to_string());
            let totp = reveal(Secret::Totp);
            let hist = v.pm_history(id).unwrap();
            json!({
                "summary": e,
                "detail": d,
                "history": hist,
                "secrets": {
                    "username": reveal(Secret::Username),
                    "password": reveal(Secret::Password),
                    "notes": reveal(Secret::Notes),
                    "totp": totp,
                    "totp_code_at": totp.as_ref().map(|u| json!({"t": TOTP_T, "code": Totp::parse(u).unwrap().code_at(TOTP_T)})),
                    "fields": (0..d.fields.len()).map(|i| reveal(Secret::Field(i))).collect::<Vec<_>>(),
                    "history": (0..hist.len()).map(|i| reveal(Secret::History(i))).collect::<Vec<_>>(),
                }
            })
        })
        .collect();
    let slots = serde_json::to_value(v.slots()).unwrap();
    let appearance_core = appearance_json(v.appearance().as_ref());
    v.close().unwrap();

    let stick = tempfile::tempdir().unwrap();
    let p = stick.path().join(s.file);
    std::fs::copy(fixtures.join(s.file), &p).unwrap();
    let st = portable(stick.path());
    let recent = st.recent_list();
    assert_eq!(recent.len(), 1);
    let r = &recent[0];
    let info = st.vault_open(p.to_str().unwrap(), pw(s.passwords[0].1)).unwrap();
    let app = json!({
        "open_password_slot": s.passwords[0].0,
        "recent_before_open": {"name": r.name, "accent": r.accent, "emblem": r.emblem, "show_when_locked": r.show_when_locked},
        "vault_info": {"name": info.name, "kdf": info.kdf, "emblem": info.emblem, "show_when_locked": info.show_when_locked,
                       "visibility": info.visibility, "settings": info.settings, "appearance_tampered": info.appearance_tampered},
        "appearance": st.vault_appearance_get(&info.id).unwrap(),
        "settings": st.settings_get(Some(&info.id)).unwrap(),
        "slots": st.slots_list(&info.id).unwrap(),
        "pm_list": st.pm_list(&info.id).unwrap(),
    });
    st.vault_lock(&info.id, "user").unwrap();
    let header_after_app_open = appearance_json(nv_core::peek_appearance(&p).unwrap().as_ref());

    json!({
        "file": s.file,
        "description": s.description,
        "size": raw.len(),
        "sha256": sha(&raw),
        "passwords": s.passwords.iter().map(|(slot, p)| json!({"slot": slot, "password": p})).collect::<Vec<_>>(),
        "duress": s.duress.map(|(slot, p)| json!({"slot": slot, "password": p})),
        "identity": s.identity.map(|(f, p, slot)| json!({"file": f, "password": p, "slot": slot})),
        "peek": {
            "appearance": appearance_json(peek.appearance.as_ref()),
            "password_kdf": peek.password_kdf,
            "recipient_slots": peek.recipient_slots,
        },
        "core": {
            "appearance": appearance_core,
            "slots": slots,
            "root_mtime": root.mtime,
            "nodes": nodes,
            "absent": s.absent,
            "meta": meta,
            "pm": pm,
        },
        "app": app,
        "header_after_app_open": header_after_app_open,
    })
}

const GRANDE_PW: &str = "grande-pw";

fn gen_grande(dir: &Path) -> Spec {
    let path = dir.join("grande.nvault");
    let v = Vault::create(&path, &pw(GRANDE_PW), KdfPreset::Fast).unwrap();
    let big = pat(91, 4_700_001);
    put(&v, ROOT, "grande.bin", &big);
    put(&v, ROOT, "piccolo.txt", b"accanto al grande\n");
    v.flush().unwrap();
    fix_mtimes(&v, 1_600_000_000_000_000_000);
    v.close().unwrap();
    let mut c = BTreeMap::new();
    c.insert("grande.bin".to_owned(), big);
    c.insert("piccolo.txt".to_owned(), b"accanto al grande\n".to_vec());
    Spec {
        file: "grande.nvault",
        description: "Single-file (upgraded) volume whose database (~4.9 MB) is larger than the 4 MiB journal region: its first \
                      4 MiB live at the old end of the file (journal section split > region). nv-core only, one file of 4.7 MB.",
        passwords: vec![(0, GRANDE_PW)],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_interrotto(dir: &Path) -> Spec {
    let src = std::env::var("NV_INTERRUPTED").expect("NV_INTERRUPTED: see the module docs");
    std::fs::copy(src, dir.join("interrotto.nvault")).unwrap();
    let mut c = BTreeMap::new();
    c.insert("tenuto.bin".to_owned(), pat(40, 700_000) );
    c.insert("piccolo.txt".to_owned(), b"resta com'era\n".to_vec());
    Spec {
        file: "interrotto.nvault",
        description: "Single-file volume (journal region 128 KiB) left by a crash mid-transaction: the in-file journal is hot and \
                      continues in its tail past the database; opening it rolls back to the committed content (tenuto.bin, \
                      piccolo.txt), the overwrite of tenuto.bin and perso.bin are gone.",
        passwords: vec![(0, "interrotto-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec!["perso.bin"],
    }
}

const FIXED_PW: &str = "fisso-pw";

fn gen_fisso(dir: &Path) -> Spec {
    let path = dir.join("fisso.nvault");
    let st = portable(dir);
    let info = st.vault_create_sized(path.to_str().unwrap(), pw(FIXED_PW), Some("fast"), Some(8 << 20)).unwrap();
    assert!(info.single_file);
    let id = info.id.clone();
    let v = st.vault(&id).unwrap();
    let mut c = BTreeMap::new();
    put(&v, ROOT, "nota.txt", b"dentro un volume a dimensione fissa\n");
    c.insert("nota.txt".to_owned(), b"dentro un volume a dimensione fissa\n".to_vec());
    let d = mkdir(&v, ROOT, "Cartella");
    put(&v, d, "dati.bin", &pat(61, 1_300_001));
    c.insert("Cartella/dati.bin".to_owned(), pat(61, 1_300_001));
    v.flush().unwrap();
    st.settings_set(patch(json!({"theme": "dark", "auto_lock_minutes": 5})), Some(&id)).unwrap();
    st.vault_appearance_set(&id, Some("ottanio".into()), Some(Emblem::Icon { id: "star".into() }), Some(Visibility::Visible), None, None).unwrap();
    v.set_slot_label(0, Some("Chiavetta")).unwrap();
    v.pm_create(EntryPatch { title: Some("Wi-Fi".into()), password: z("fisso-wifi"), ..Default::default() }).unwrap();
    fix_mtimes(&v, 1_710_000_000_000_000_000);
    drop(v);
    st.vault_lock(&id, "user").unwrap();
    Spec {
        file: "fisso.nvault",
        description: "Fixed-size (8 MiB) single-file volume made through the app (vault_create_sized): header fixed + journal \
                      sections, journal region 1 MiB in place of the footer region; icon emblem 'star' visible, accent ottanio, \
                      theme dark, auto-lock 5 min; labelled slot; one PM entry; a folder with a 1.3 MB file.",
        passwords: vec![(0, FIXED_PW)],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_fisso_vecchio(dir: &Path) -> Spec {
    let path = dir.join("fisso-vecchio.nvault");
    let v = Vault::create_fixed_as(&path, &pw("fisso-vecchio-pw"), KdfPreset::Fast, 6 << 20, false).unwrap();
    assert!(!v.is_single_file());
    put(&v, ROOT, "vecchio.txt", b"journal accanto al volume\n");
    put(&v, ROOT, "medio.bin", &pat(62, 900_000));
    v.flush().unwrap();
    fix_mtimes(&v, 1_720_000_000_000_000_000);
    v.close().unwrap();
    let mut c = BTreeMap::new();
    c.insert("vecchio.txt".to_owned(), b"journal accanto al volume\n".to_vec());
    c.insert("medio.bin".to_owned(), pat(62, 900_000));
    Spec {
        file: "fisso-vecchio.nvault",
        description: "Fixed-size (6 MiB) volume with the SIDE-FILE journal and the footer region (header fixed, no journal section), \
                      as made by builds before fixed-size single-file volumes (same code path); not upgraded. nv-core only.",
        passwords: vec![(0, "fisso-vecchio-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_fisso_aggiornato(dir: &Path) -> Spec {
    let path = dir.join("fisso-aggiornato.nvault");
    let v = Vault::create_fixed_as(&path, &pw("fisso-aggiornato-pw"), KdfPreset::Fast, 8 << 20, false).unwrap();
    put(&v, ROOT, "grande.bin", &pat(63, 2_100_000));
    put(&v, ROOT, "piccolo.txt", b"aggiornato sul posto\n");
    v.flush().unwrap();
    fix_mtimes(&v, 1_730_000_000_000_000_000);
    assert!(v.upgrade_format().unwrap());
    v.close().unwrap();
    let mut c = BTreeMap::new();
    c.insert("grande.bin".to_owned(), pat(63, 2_100_000));
    c.insert("piccolo.txt".to_owned(), b"aggiornato sul posto\n".to_vec());
    Spec {
        file: "fisso-aggiornato.nvault",
        description: "Fixed-size (8 MiB) side-file volume upgraded in place to single-file: its database (~2.2 MB) is larger than \
                      the 1 MiB region's displaced part, whose bytes moved past the database (journal section split > region - 64 KiB). \
                      nv-core only.",
        passwords: vec![(0, "fisso-aggiornato-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_fisso_esteso(dir: &Path) -> Spec {
    let path = dir.join("fisso-esteso.nvault");
    let v = Vault::create_fixed(&path, &pw("fisso-esteso-pw"), KdfPreset::Fast, 6 << 20).unwrap();
    put(&v, ROOT, "prima.bin", &pat(64, 1_500_000));
    v.flush().unwrap();
    assert_eq!(v.resize(10 << 20, &|_| {}).unwrap(), 10 << 20);
    put(&v, ROOT, "dopo.bin", &pat(65, 3_000_000));
    v.flush().unwrap();
    fix_mtimes(&v, 1_740_000_000_000_000_000);
    v.close().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 10 << 20);
    let mut c = BTreeMap::new();
    c.insert("prima.bin".to_owned(), pat(64, 1_500_000));
    c.insert("dopo.bin".to_owned(), pat(65, 3_000_000));
    Spec {
        file: "fisso-esteso.nvault",
        description: "Fixed-size single-file volume made at 6 MiB (region 512 KiB), extended to 10 MiB (Estendi: journal records of \
                      the new capacity, header capacity 10 MiB), then written past the old size. nv-core only.",
        passwords: vec![(0, "fisso-esteso-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec![],
    }
}

fn gen_fisso_interrotto(dir: &Path) -> Spec {
    let src = std::env::var("NV_INTERRUPTED_FIXED").expect("NV_INTERRUPTED_FIXED: see the module docs");
    std::fs::copy(src, dir.join("fisso-interrotto.nvault")).unwrap();
    let mut c = BTreeMap::new();
    c.insert("tenuto.bin".to_owned(), pat(40, 700_000));
    c.insert("piccolo.txt".to_owned(), b"resta com'era\n".to_vec());
    c.insert("pieno/riempitivo.bin".to_owned(), pat(52, 3_400_000));
    Spec {
        file: "fisso-interrotto.nvault",
        description: "Fixed-size (6 MiB) single-file volume (region 128 KiB) whose store reaches almost to its capacity, left by a \
                      crash mid-transaction: the hot journal's tail lies PAST the capacity (the file is longer than its 6 MiB); \
                      opening it rolls back to tenuto.bin + piccolo.txt + pieno/riempitivo.bin (no perso.bin, no pieno/liberato.bin) \
                      and cuts the file back to 6 MiB.",
        passwords: vec![(0, "fisso-interrotto-pw")],
        duress: None,
        identity: None,
        contents: c,
        absent: vec!["perso.bin", "pieno/liberato.bin"],
    }
}

fn upgrade(dir: &Path, s: &Spec) {
    let v = Vault::open(&dir.join(s.file), &pw(s.passwords[0].1)).unwrap();
    assert!(v.upgrade_format().unwrap(), "{}", s.file);
    v.close().unwrap();
}

fn rand_name() -> String {
    nicevault_app::state::random_id()
}

#[test]
#[ignore = "generates a fixture set; run by hand once per release (see the module docs)"]
fn generate_compat_fixtures() {
    let out = std::env::var("NV_COMPAT_OUT").expect("set NV_COMPAT_OUT to the new fixture folder");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out: PathBuf = if Path::new(&out).is_absolute() { out.into() } else { root.join(out) };
    assert!(!out.exists() || std::fs::read_dir(&out).unwrap().next().is_none(), "{} exists: fixture sets are never rewritten", out.display());
    std::fs::create_dir_all(&out).unwrap();

    let work = tempfile::tempdir().unwrap();
    let single = std::env::var("NV_COMPAT_SINGLE").is_ok_and(|v| v == "1");
    let fixed = std::env::var("NV_COMPAT_FIXED").is_ok_and(|v| v == "1");
    let mut specs = if fixed {
        vec![gen_fisso(work.path()), gen_fisso_vecchio(work.path()), gen_fisso_aggiornato(work.path()), gen_fisso_esteso(work.path()), gen_fisso_interrotto(work.path())]
    } else {
        vec![gen_archivio(work.path()), gen_cifrato(work.path()), gen_bordeaux(work.path()), gen_nudo(work.path())]
    };
    if single {
        specs.push(gen_grande(work.path()));
        for s in &specs {
            upgrade(work.path(), s);
        }
        specs.push(gen_interrotto(work.path()));
    }
    for s in &specs {
        std::fs::copy(work.path().join(s.file), out.join(s.file)).unwrap();
        if let Some((f, ..)) = s.identity {
            std::fs::copy(work.path().join(f), out.join(f)).unwrap();
        }
    }
    let volumes: Vec<Value> = specs.iter().map(|s| record(&out, s)).collect();
    let manifest = json!({
        "manifest_version": 1,
        "generated_by_commit": std::env::var("NV_COMPAT_COMMIT").unwrap_or_else(|_| "unknown".into()),
        "generated_on": out.file_name().unwrap().to_string_lossy(),
        "kdf_note": "All slots use KdfPreset::Fast (Argon2id m=8 MiB, t=1, p=1) for test speed, except nudo.nvault which uses \
                     KdfPreset::Strong as calibrated on the generating machine (p = 4, like real volumes).",
        "totp_t": TOTP_T,
        "chunk": CHUNK,
        "volumes": volumes,
    });
    std::fs::write(out.join("manifest.json"), serde_json::to_string_pretty(&manifest).unwrap() + "\n").unwrap();
    let total: u64 = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
    println!("fixture set {} written: {total} bytes", out.display());
    assert!(total < if single || fixed { 40 << 20 } else { 5 << 20 }, "fixture set too large: {total} bytes");
}
