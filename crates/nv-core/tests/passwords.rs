use keepass::db::{Entry, fields};
use keepass::{Database, DatabaseKey};
use nv_core::passwords::import::{self, ImportError};
use nv_core::passwords::totp::{Algorithm, Totp};
use nv_core::passwords::{ImportedEntry, Secret};
use nv_core::{KdfPreset, Vault};
use secrecy::SecretString;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn by_title<'a>(v: &'a [ImportedEntry], t: &str) -> &'a ImportedEntry {
    v.iter().find(|e| e.entry.title.as_deref() == Some(t)).unwrap_or_else(|| panic!("{t} missing"))
}

fn s(z: &Option<zeroize::Zeroizing<String>>) -> &str {
    z.as_deref().map(String::as_str).unwrap_or("")
}

fn field<'a>(e: &'a ImportedEntry, name: &str) -> (&'a str, bool) {
    let f = e.entry.fields.as_ref().and_then(|f| f.iter().find(|f| f.name == name)).unwrap_or_else(|| panic!("field {name} missing"));
    (s(&f.value), f.hidden)
}

#[test]
fn bitwarden_json_export() {
    let v = import::bitwarden_json(&fixture("bitwarden.json")).unwrap();
    assert_eq!(v.len(), 4);
    let mail = by_title(&v, "Posta aziendale");
    assert_eq!(mail.entry.username.as_deref(), Some("anna@example.com"));
    assert_eq!(s(&mail.entry.password), "bw-current-pass-1");
    assert_eq!(mail.entry.urls.as_ref().unwrap().len(), 2);
    assert_eq!(mail.entry.folder.as_deref(), Some("Lavoro"));
    assert_eq!(mail.entry.favorite, Some(true));
    assert!(s(&mail.entry.totp).starts_with("otpauth://totp/"));
    assert_eq!(field(mail, "Risposta"), ("bw-hidden-answer-77", true));
    assert_eq!(field(mail, "Domanda"), ("Colore preferito", false));
    assert_eq!(mail.history.len(), 2);
    assert_eq!(mail.created, Some(1_672_531_200_000));
    let forum = by_title(&v, "forum.example.org");
    assert!(forum.entry.totp.is_none());
    assert_eq!(field(forum, "TOTP"), ("not a secret!", true));
    assert_eq!(s(&by_title(&v, "Codice allarme").entry.notes), "1234#");
    let card = by_title(&v, "Carta");
    assert_eq!(field(card, "number"), ("4111111111111111", true));
    assert_eq!(field(card, "brand"), ("Visa", false));

    assert_eq!(import::bitwarden_json(br#"{"encrypted":true,"items":[]}"#).err(), Some(ImportError::Encrypted));
    assert!(matches!(import::bitwarden_json(b"[1,2]"), Err(ImportError::Format(_))));
}

#[test]
fn csv_exports() {
    let chrome = import::csv(&fixture("chrome.csv")).unwrap();
    assert_eq!(chrome.len(), 2);
    assert_eq!(s(&by_title(&chrome, "Example").entry.password), "chrome-pass-1");
    assert_eq!(s(&by_title(&chrome, "Example").entry.notes), "first note");
    assert_eq!(by_title(&chrome, "nameless.example.net").entry.username.as_deref(), Some("nobody"));

    let ff = import::csv(&fixture("firefox.csv")).unwrap();
    assert_eq!(ff.len(), 1);
    let e = by_title(&ff, "accounts.firefox.example");
    assert_eq!(s(&e.entry.password), "ff-pass-1");
    assert_eq!(e.created, Some(1_700_000_000_000));
    assert!(e.entry.fields.is_none(), "Firefox bookkeeping columns are dropped");

    let op = import::csv(&fixture("onepassword.csv")).unwrap();
    let b = by_title(&op, "Banca");
    assert_eq!(b.entry.favorite, Some(true));
    assert_eq!(b.entry.tags.as_deref(), Some(&["soldi".to_owned(), "importante".to_owned()][..]));
    assert_eq!(s(&b.entry.notes), "multi\nline note");
    assert_eq!(field(b, "Security question"), ("nome del cane", false));
    let t = Totp::parse(s(&b.entry.totp)).unwrap();
    assert_eq!((t.digits, t.period), (8, 60));

    let semi = import::csv("title;username;password\nSito;io;pw-semi\n".as_bytes()).unwrap();
    assert_eq!(s(&semi[0].entry.password), "pw-semi");
    assert!(matches!(import::csv(b"a,b\n1,2\n"), Err(ImportError::Format(_))));
}

#[test]
fn keepass_kdbx31_fixture() {
    let data = fixture("kdbx31_demopass.kdbx");
    assert!(matches!(import::keepass(&data, Some("wrong"), None), Err(ImportError::WrongKey)));
    let v = import::keepass(&data, Some("demopass"), None).unwrap();
    assert!(!v.is_empty());
    let e = by_title(&v, "Sample Entry");
    assert_eq!(e.entry.username.as_deref(), Some("User Name"));
    assert_eq!(s(&e.entry.password), "Password");
    assert_eq!(by_title(&v, "test entry").entry.folder.as_deref(), Some("General/Subgroup"));
    assert_eq!(by_title(&v, "asdf").entry.folder.as_deref(), Some("Internet"));
}

#[test]
fn keepass_kdbx4_written_here() {
    let mut db = Database::new();
    let bin_id;
    {
        let mut root = db.root_mut();
        let mut g = root.add_group();
        g.name = "Banche".into();
        let mut sub = g.add_group();
        sub.name = "Estero".into();
        let mut e = sub.add_entry();
        e.set_unprotected(fields::TITLE, "Conto CH");
        e.set_unprotected(fields::USERNAME, "kp-user");
        e.set_protected(fields::PASSWORD, "kp-old-pass");
        let snapshot: Entry = (*e).clone();
        e.set_protected(fields::PASSWORD, "kp-new-pass");
        e.history.get_or_insert_default().add_entry(snapshot);
        e.set_unprotected(fields::URL, "https://bank.example.ch");
        e.set_unprotected("KP2A_URL_1", "https://app.bank.example.ch");
        e.set_protected(fields::OTP, "otpauth://totp/Bank:kp-user?secret=JBSWY3DPEHPK3PXP&issuer=Bank");
        e.set_protected("PIN", "4242");
        e.set_unprotected("Filiale", "Lugano");
        e.tags.push("conto".into());
    }
    {
        let mut root = db.root_mut();
        let mut e = root.add_entry();
        e.set_unprotected(fields::TITLE, "Server");
        e.set_unprotected(fields::USERNAME, "root");
        e.set_protected(fields::PASSWORD, "kp-root");
        e.set_protected("TimeOtp-Secret-Base32", "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        e.set_unprotected("TimeOtp-Algorithm", "HMAC-SHA-256");
        e.set_unprotected("TimeOtp-Length", "8");
        e.set_unprotected("TimeOtp-Period", "60");
        let mut bin = root.add_group();
        bin.name = "Cestino".into();
        bin_id = bin.id();
        bin.add_entry().set_unprotected(fields::TITLE, "Eliminato");
    }
    db.meta.recyclebin_uuid = Some(bin_id.uuid());
    let keyfile = b"any bytes work as a KeePass key file".to_vec();
    let mut buf = Vec::new();
    db.save(&mut buf, DatabaseKey::new().with_password("kp-master").with_keyfile(&mut keyfile.as_slice()).unwrap()).unwrap();

    assert!(matches!(import::keepass(&buf, Some("kp-master"), None), Err(ImportError::WrongKey)), "key file required");
    let v = import::keepass(&buf, Some("kp-master"), Some(&keyfile)).unwrap();
    assert_eq!(v.len(), 2, "recycle bin skipped: {:?}", v.iter().map(|e| e.entry.title.clone()).collect::<Vec<_>>());
    let ch = by_title(&v, "Conto CH");
    assert_eq!(ch.entry.folder.as_deref(), Some("Banche/Estero"));
    assert_eq!(s(&ch.entry.password), "kp-new-pass");
    assert_eq!(ch.entry.urls.as_deref(), Some(&["https://bank.example.ch".to_owned(), "https://app.bank.example.ch".to_owned()][..]));
    assert_eq!(field(ch, "PIN"), ("4242", true));
    assert_eq!(field(ch, "Filiale"), ("Lugano", false));
    assert_eq!(Totp::parse(s(&ch.entry.totp)).unwrap().issuer.as_deref(), Some("Bank"));
    assert_eq!(ch.history.len(), 1);
    assert_eq!(&*ch.history[0].0, "kp-old-pass");
    assert_eq!(ch.entry.tags.as_deref(), Some(&["conto".to_owned()][..]));

    let srv = by_title(&v, "Server");
    assert_eq!(srv.entry.folder.as_deref(), Some(""));
    let t = Totp::parse(s(&srv.entry.totp)).unwrap();
    assert_eq!((t.algorithm, t.digits, t.period), (Algorithm::Sha256, 8, 60));
    let direct = Totp::new(b"12345678901234567890".to_vec(), Algorithm::Sha256, 8, 60).unwrap();
    assert_eq!(t.code_at(1_234_567_890), direct.code_at(1_234_567_890));
    assert!(srv.entry.fields.is_none(), "TimeOtp-* fields are consumed");

    let tmp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&tmp.path().join("kp.nvault"), &SecretString::from("pw".to_owned()), KdfPreset::Fast).unwrap();
    let ids = vault.pm_insert_many(v).unwrap();
    assert_eq!(ids.len(), 2);
    let list = vault.pm_list().unwrap();
    let ch = list.iter().find(|e| e.title == "Conto CH").unwrap();
    assert_eq!(ch.folder, "Banche/Estero");
    assert!(ch.has_totp);
    assert_eq!(vault.pm_history(&ch.id).unwrap().len(), 1);
    assert_eq!(&*vault.pm_reveal(&ch.id, Secret::History(0)).unwrap(), "kp-old-pass");
    assert_eq!(vault.pm_totp(&ch.id).unwrap().digits, 6);
    vault.close().unwrap();
}

fn urls(e: &ImportedEntry) -> Vec<&str> {
    e.entry.urls.as_deref().unwrap_or_default().iter().map(String::as_str).collect()
}

fn field_names(e: &ImportedEntry) -> Vec<&str> {
    e.entry.fields.as_deref().unwrap_or_default().iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn csv_known_fields_are_mapped() {
    let v = import::csv(&fixture("autofill.csv")).unwrap();
    let ms = by_title(&v, "login.microsoftonline.com");
    assert_eq!(urls(ms), ["https://login.microsoftonline.com/", "https://account.example.com/"]);
    assert_eq!((ms.created, ms.updated), (Some(1_774_863_307_000), Some(1_790_593_026_000)));
    assert!(ms.entry.fields.is_none(), "{:?}", field_names(ms));
    assert_eq!(by_title(&v, "Forum").created, Some(1_700_000_000_000));
    assert_eq!(by_title(&v, "Forum").updated, Some(1_700_000_000_000));

    let v = import::csv(&fixture("protonpass.csv")).unwrap();
    let pm = by_title(&v, "Proton Mail");
    assert_eq!(pm.entry.username.as_deref(), Some("mario@proton.example"), "e-mail stands in for an empty user name");
    assert_eq!((pm.created, pm.updated), (Some(1_714_000_000_000), Some(1_714_500_000_000)));
    assert!(s(&pm.entry.totp).starts_with("otpauth://totp/"));
    assert!(pm.entry.fields.is_none());
    let g = by_title(&v, "Gioco");
    assert_eq!(g.entry.username.as_deref(), Some("mario99"));
    assert_eq!(field(g, "email"), ("mario@proton.example", false), "a different e-mail is kept");

    let v = import::csv(&fixture("keepassxc.csv")).unwrap();
    let c = by_title(&v, "Conto");
    assert_eq!(c.entry.folder.as_deref(), Some("Banche"), "root group name dropped");
    assert_eq!(c.created, Some(1_673_771_400_000));
    assert_eq!(c.updated, Some(1_709_287_200_000));
    assert!(c.entry.fields.is_none(), "Icon numbers are noise: {:?}", field_names(c));
    assert_eq!(by_title(&v, "Radice").entry.folder.as_deref(), Some(""));

    let v = import::csv(&fixture("dashlane.csv")).unwrap();
    let d = by_title(&v, "Negozio");
    assert!(s(&d.entry.totp) == "JBSWY3DPEHPK3PXP", "otpSecret becomes the 2FA code");
    assert_eq!(d.entry.folder.as_deref(), Some("Acquisti"));
    assert_eq!(field_names(d), ["username2"]);

    let v = import::csv(&fixture("nordpass.csv")).unwrap();
    let p = by_title(&v, "Posta");
    assert_eq!(urls(p), ["https://mail.example.net", "https://webmail.example.net", "https://m.example.net"]);
    assert_eq!(p.entry.username.as_deref(), Some("np-user"));
    assert_eq!(field(p, "email"), ("np-user@example.net", false));
    let card = by_title(&v, "Carta");
    assert_eq!(field(card, "cardnumber"), ("4111111111111111", true));
    assert_eq!(field(card, "cvc"), ("123", true));
    assert_eq!(field(card, "pin"), ("9876", true));
    assert_eq!(field(card, "cardholdername"), ("Mario Rossi", false));

    let v = import::csv(&fixture("lastpass.csv")).unwrap();
    assert_eq!(by_title(&v, "LastPass sito").entry.favorite, Some(true));
    let note = by_title(&v, "Nota sicura");
    assert!(urls(note).is_empty(), "http://sn is LastPass' secure-note marker");
    assert!(s(&note.entry.notes).contains("Hostname:srv"));

    let v = import::csv(&fixture("bitwarden.csv")).unwrap();
    let git = by_title(&v, "Git");
    assert_eq!(urls(git), ["https://git.example.com", "https://ci.example.com"]);
    assert_eq!(field(git, "Team"), ("core", false));
    assert_eq!(git.entry.folder.as_deref(), Some("Lavoro"));

    let v = import::csv(&fixture("safari.csv")).unwrap();
    assert!(by_title(&v, "Apple sito (sa-user)").entry.totp.is_some());

    let v = import::csv(&fixture("roboform.csv")).unwrap();
    let rf = by_title(&v, "Banca RF");
    assert_eq!(s(&rf.entry.password), "rf-pass-1", "Pwd is the password");
    assert_eq!(rf.entry.username.as_deref(), Some("rf-user"));
    assert_eq!(urls(rf), ["https://rf.example.com/login", "https://rf.example.com/"]);
    assert!(rf.entry.fields.is_none(), "{:?}", field_names(rf));

    let ff = import::csv(&fixture("firefox.csv")).unwrap();
    assert_eq!(ff[0].updated, Some(1_700_000_000_000));
}

#[test]
fn json_and_keepass_custom_fields_are_mapped() {
    let json = br#"{"encrypted":false,"folders":[],"items":[{"type":1,"name":"Sito","login":{"username":"u","password":"p","uris":[{"uri":"https://a.example"}]},
      "fields":[{"name":"AUTOFILLURLS","value":"[{\"url\":\"https://b.example\",\"mode\":0}]","type":0},{"name":"createTime","value":"1714000000","type":0},
      {"name":"Collegato","value":null,"type":3,"linkedId":100},{"name":"Altro","value":"x","type":0}]}]}"#;
    let v = import::bitwarden_json(json).unwrap();
    assert_eq!(urls(&v[0]), ["https://a.example", "https://b.example"]);
    assert_eq!(v[0].created, Some(1_714_000_000_000));
    assert_eq!(field_names(&v[0]), ["Altro"], "linked fields are references, not values");

    let mut db = Database::new();
    {
        let mut root = db.root_mut();
        let mut e = root.add_entry();
        e.set_unprotected(fields::TITLE, "KP");
        e.set_unprotected("_etm_template_uuid", "abcd");
        e.set_unprotected("KeePassHttp Settings", "{\"Allow\":[]}");
        e.set_unprotected("URLS", "https://x.example https://y.example");
        e.set_unprotected("Nota", "resta");
    }
    let mut buf = Vec::new();
    db.save(&mut buf, DatabaseKey::new().with_password("k")).unwrap();
    let v = import::keepass(&buf, Some("k"), None).unwrap();
    assert_eq!(urls(&v[0]), ["https://x.example", "https://y.example"]);
    assert_eq!(field_names(&v[0]), ["Nota"]);
}

fn raw_fields(v: &Vault, id: &str) -> Vec<String> {
    v.pm_get(id).unwrap().fields.iter().map(|f| f.name.clone()).collect()
}

#[test]
fn repair_converts_raw_import_fields() {
    use nv_core::passwords::{EntryPatch, FieldInput, REPAIR_KEY};
    use zeroize::Zeroizing;
    let z = |s: &str| Some(Zeroizing::new(s.to_owned()));
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("r.nvault");
    let v = Vault::create(&path, &SecretString::from("pw".to_owned()), KdfPreset::Fast).unwrap();
    let raw = |name: &str, value: &str| FieldInput { name: name.into(), value: z(value), hidden: false };
    let ms = v
        .pm_create(EntryPatch {
            title: Some("login.microsoftonline.com".into()),
            password: z("ms-pass-1"),
            urls: Some(vec!["https://login.microsoftonline.com/".into()]),
            fields: Some(vec![
                raw("AUTOFILLURLS", r#"[{"url":"https://login.microsoftonline.com/","mode":0},{"url":"https://b.example/","mode":0}]"#),
                raw("CREATETIME", "1774863307"),
                raw("MODIFYTIME", "1790593026"),
                raw("Domanda", "colore"),
                raw("createTime", "non una data"),
            ]),
            ..Default::default()
        })
        .unwrap()
        .summary
        .id;
    let otp = v
        .pm_create(EntryPatch { title: Some("Dashlane".into()), fields: Some(vec![raw("otpSecret", "JBSWY3DPEHPK3PXP"), raw("Icon", "3")]), ..Default::default() })
        .unwrap()
        .summary
        .id;
    let plain = v.pm_create(EntryPatch { title: Some("Normale".into()), fields: Some(vec![raw("PIN", "1234")]), ..Default::default() }).unwrap();
    v.meta_set(REPAIR_KEY, None).unwrap();
    v.close().unwrap();

    let v = Vault::open(&path, &SecretString::from("pw".to_owned())).unwrap();
    assert_eq!(v.meta_get(REPAIR_KEY).unwrap().as_deref(), Some(&b"1"[..]));
    let d = v.pm_get(&ms).unwrap();
    assert_eq!(d.summary.urls, ["https://login.microsoftonline.com/", "https://b.example/"]);
    assert_eq!((d.summary.created, d.summary.updated), (1_774_863_307_000, 1_790_593_026_000));
    assert_eq!(raw_fields(&v, &ms), ["Domanda", "createTime"], "unparseable values stay");
    assert_eq!(&*v.pm_reveal(&ms, Secret::Password).unwrap(), "ms-pass-1");
    let d = v.pm_get(&otp).unwrap();
    assert!(d.summary.has_totp);
    assert_eq!(raw_fields(&v, &otp), ["Icon"], "the repair converts, it does not drop noise");
    let p = v.pm_get(&plain.summary.id).unwrap();
    assert_eq!(p, plain, "an entry without import fields is untouched");
    assert_eq!(p.fields[0].value.as_deref(), Some("1234"), "the repair never hides a field");

    let before = serde_json::to_string(&v.pm_list().unwrap()).unwrap();
    assert_eq!(v.pm_repair_imported().unwrap(), 0);
    assert_eq!(serde_json::to_string(&v.pm_list().unwrap()).unwrap(), before);
    v.close().unwrap();
}

#[test]
fn folders_move_rename_delete() {
    use nv_core::passwords::{EntryPatch, PmError};
    let tmp = tempfile::tempdir().unwrap();
    let v = Vault::create(&tmp.path().join("f.nvault"), &SecretString::from("pw".to_owned()), KdfPreset::Fast).unwrap();
    let mk = |t: &str, f: &str| v.pm_create(EntryPatch { title: Some(t.into()), folder: Some(f.into()), ..Default::default() }).unwrap().summary;
    let a = mk("a", "Lavoro");
    let b = mk("b", "Lavoro/Clienti");
    let c = mk("c", "Lavoretti");
    let d = mk("d", "");
    let folder = |id: &str| v.pm_get(id).unwrap().summary.folder;

    assert_eq!(v.pm_move(&[d.id.clone(), c.id.clone(), "nope".into()], " Casa / Rete ").unwrap(), 2);
    assert_eq!(folder(&d.id), "Casa/Rete");
    assert_eq!(v.pm_get(&d.id).unwrap().summary.updated, d.updated, "a move is not an edit");
    assert_eq!(v.pm_move(std::slice::from_ref(&d.id), "Casa/Rete").unwrap(), 0, "already there");

    v.pm_move(std::slice::from_ref(&c.id), "Lavoretti").unwrap();
    assert_eq!(v.pm_folder_rename("Lavoro", "Ufficio").unwrap(), 2);
    assert_eq!((folder(&a.id), folder(&b.id), folder(&c.id)), ("Ufficio".into(), "Ufficio/Clienti".into(), "Lavoretti".into()));
    assert!(matches!(v.pm_folder_rename("Inesistente", "X"), Err(PmError::NotFound)));
    assert!(matches!(v.pm_folder_rename("Ufficio", " / "), Err(PmError::Invalid(_))));

    assert_eq!(v.pm_folder_delete("Ufficio").unwrap(), 2);
    assert_eq!((folder(&a.id), folder(&b.id)), ("".into(), "Clienti".into()));
    v.pm_move(std::slice::from_ref(&b.id), "Casa/Rete/Sotto").unwrap();
    assert_eq!(v.pm_folder_delete("Casa/Rete").unwrap(), 2);
    assert_eq!((folder(&d.id), folder(&b.id)), ("Casa".into(), "Casa/Sotto".into()));
    v.close().unwrap();
}

#[test]
fn site_icons_are_stored_in_the_volume() {
    use nv_core::passwords::{EntryPatch, icon_host};
    let h = |u: &str| icon_host(&[u.to_owned()]);
    assert_eq!(h("https://Login.Example.com:8443/x?y"), Some("login.example.com".into()));
    assert_eq!(h("example.org/path"), Some("example.org".into()));
    assert_eq!(h("http://192.168.1.1"), None);
    assert_eq!(h("https://nas.local"), None);
    assert_eq!(h("https://localhost:3000"), None);
    assert_eq!(h("ftp://files.example.com"), None);
    assert_eq!(h("https://[::1]/"), None);
    assert_eq!(icon_host(&["http://10.0.0.1".into(), "https://second.example".into()]), Some("second.example".into()));

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("i.nvault");
    let v = Vault::create(&path, &SecretString::from("pw".to_owned()), KdfPreset::Fast).unwrap();
    let e = v.pm_create(EntryPatch { title: Some("Sito".into()), urls: Some(vec!["https://www.example.com/login".into()]), ..Default::default() }).unwrap();
    v.pm_create(EntryPatch { title: Some("Router".into()), urls: Some(vec!["http://192.168.1.1".into()]), ..Default::default() }).unwrap();
    assert_eq!(e.summary.icon, None);
    assert_eq!(v.pm_icon_hosts().unwrap(), [("www.example.com".to_owned(), None)], "no table yet: nothing fetched");
    let png = b"\x89PNG\r\n\x1a\nfake".to_vec();
    assert!(v.pm_icon_put("www.example.com", Some(b"<svg/>"), 5).is_err(), "only PNG is stored");
    v.pm_icon_put("www.example.com", Some(&png), 1000).unwrap();
    assert_eq!(v.pm_list().unwrap().iter().find(|s| s.id == e.summary.id).unwrap().icon.as_deref(), Some("www.example.com/1000"));
    assert_eq!(v.pm_get(&e.summary.id).unwrap().summary.icon.as_deref(), Some("www.example.com/1000"));
    v.pm_icon_put("www.example.com", None, 2000).unwrap();
    assert_eq!(v.pm_icon_get("www.example.com").unwrap(), png);
    assert_eq!(v.pm_icon_hosts().unwrap(), [("www.example.com".to_owned(), Some(2000))]);
    v.pm_icon_put("none.example", None, 3000).unwrap();
    assert!(v.pm_icon_get("none.example").is_err());
    v.close().unwrap();
    let raw = std::fs::read(&path).unwrap();
    assert!(!raw.windows(8).any(|w| w == b"fake\x00\x00\x00\x00" || w == b"example."));
}
