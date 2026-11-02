use nicevault_app::store::{Locations, Store};
use nicevault_app::{AppState, NoEvents};
use secrecy::SecretString;
use std::path::Path;
use std::sync::Arc;

fn pw(s: &str) -> SecretString {
    SecretString::from(s.to_owned())
}

fn app(config: Option<&Path>) -> Arc<AppState> {
    AppState::new(Store::new(Locations { exe_dir: None, removable: Some(vec![]), config_dir: config.map(Path::to_path_buf) }), Box::new(NoEvents))
}

fn code<T: std::fmt::Debug>(r: Result<T, nicevault_app::error::ApiError>) -> String {
    r.unwrap_err().code
}

#[test]
fn my_key_lifecycle_and_sharing() {
    let t = tempfile::tempdir().unwrap();
    let (cfg_a, cfg_b) = (t.path().join("cfg-a"), t.path().join("cfg-b"));
    let alice = app(Some(&cfg_a));

    let k = alice.my_key_get().unwrap();
    assert_eq!((k.public.clone(), k.persisted), (None, true));
    assert_eq!(code(alice.my_key_create(pw(""), Some("fast"))), "invalid_password");
    let mine = alice.my_key_create(pw("chiave"), Some("fast")).unwrap();
    let public = mine.public.clone().unwrap();
    assert!(public.starts_with("nvpk1"));
    assert_eq!(mine.fingerprint.as_deref().map(str::len), Some(32));
    assert_eq!(code(alice.my_key_create(pw("x"), Some("fast"))), "exists");
    let file = cfg_a.join(nicevault_app::mykey::FILE);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(json["public"], public.as_str());
    assert!(!json.to_string().contains("chiave"), "the password is never stored");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert_eq!(app(Some(&cfg_a)).my_key_get().unwrap(), mine, "read back after a restart");

    let nvkey = t.path().join("me.nvkey");
    let nvpub = t.path().join("me.nvpub");
    alice.my_key_export(nvkey.to_str().unwrap()).unwrap();
    assert_eq!(code(alice.my_key_export(nvkey.to_str().unwrap())), "exists", "never over a file");
    alice.my_key_export_public(nvpub.to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&nvpub).unwrap().trim(), public);

    let other = app(Some(&cfg_b));
    assert_eq!(code(other.my_key_import(nvkey.to_str().unwrap(), pw("nope"))), "wrong_password");
    assert_eq!(code(other.my_key_import(nvpub.to_str().unwrap(), pw("chiave"))), "not_a_key");
    assert_eq!(other.my_key_import(nvkey.to_str().unwrap(), pw("chiave")).unwrap().public, Some(public.clone()));
    assert!(other.my_key_import(nvkey.to_str().unwrap(), pw("chiave")).is_ok(), "same key again: no-op");

    let wrapped = public.as_bytes().chunks(64).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("\n");
    assert_eq!(alice.recipient_check(&format!("  {wrapped}\n")).unwrap().public, public);
    assert_eq!(alice.recipient_read_file(nvpub.to_str().unwrap()).unwrap().fingerprint, mine.fingerprint.clone().unwrap());
    assert_eq!(code(alice.recipient_read_file(nvkey.to_str().unwrap())), "secret_key_file");
    assert_eq!(code(alice.recipient_check("nvpk1abc")), "invalid_recipient");
    let mut broken = public.clone();
    broken.replace_range(20..21, if &public[20..21] == "A" { "B" } else { "A" });
    assert_eq!(code(alice.recipient_check(&broken)), "invalid_recipient", "checksum");

    let vol = t.path().join("condiviso.nvault");
    let vid = alice.vault_create_sized(vol.to_str().unwrap(), pw("pw"), Some("fast"), None).unwrap().id;
    let anna = app(Some(&t.path().join("cfg-anna")));
    let anna_pub = anna.my_key_create(pw("anna"), Some("fast")).unwrap().public.unwrap();
    let slots = alice.slot_add_recipient(&vid, &public, Some(" Io ".into())).unwrap();
    let me_slot = slots.iter().find(|s| s.kind == "recipient").unwrap().clone();
    assert_eq!((me_slot.label.as_deref(), me_slot.fingerprint.clone()), (Some("Io"), mine.fingerprint.clone()));
    assert_eq!(code(alice.slot_add_recipient(&vid, &public, None)), "recipient_exists");
    assert_eq!(code(alice.slot_add_recipient(&vid, "nvpk1zz", None)), "invalid_recipient");
    let slots = alice.slot_add_recipient(&vid, &anna_pub, Some("Anna".into())).unwrap();
    let anna_slot = slots.iter().find(|s| s.label.as_deref() == Some("Anna")).unwrap().index;
    alice.vault_lock(&vid, "user").unwrap();

    let r = alice.recent_list().into_iter().find(|r| Path::new(&r.path) == vol).unwrap();
    assert_eq!(r.recipients.len(), 2);
    assert!(r.recipients.contains(mine.fingerprint.as_ref().unwrap()));

    assert_eq!(code(alice.vault_open_with_key(vol.to_str().unwrap(), pw("sbagliata"))), "wrong_password");
    assert_eq!(code(app(Some(&t.path().join("cfg-none"))).vault_open_with_key(vol.to_str().unwrap(), pw("x"))), "no_key");
    let info = other.vault_open_with_key(vol.to_str().unwrap(), pw("chiave")).unwrap();
    let in_use: Vec<_> = other.slots_list(&info.id).unwrap().into_iter().filter(|s| s.in_use).map(|s| s.index).collect();
    assert_eq!(in_use, [me_slot.index]);
    other.vault_lock(&info.id, "user").unwrap();
    let stranger = app(Some(&t.path().join("cfg-c")));
    stranger.my_key_create(pw("c"), Some("fast")).unwrap();
    assert_eq!(code(stranger.vault_open_with_key(vol.to_str().unwrap(), pw("c"))), "not_recipient");

    let vid = alice.vault_open(vol.to_str().unwrap(), pw("pw")).unwrap().id;
    let after = alice.slot_rotate_key(&vid, 0, pw("pw")).unwrap();
    let mut labels: Vec<_> = after.iter().map(|s| (s.kind, s.label.clone())).collect();
    labels.sort();
    assert_eq!(labels, [("password", None), ("recipient", Some("Anna".into())), ("recipient", Some("Io".into()))]);
    let anna_slot = after.iter().find(|s| s.label.as_deref() == Some("Anna")).map_or(anna_slot, |s| s.index);
    alice.slot_remove(&vid, anna_slot).unwrap();
    let after = alice.slot_rotate_key(&vid, 0, pw("pw")).unwrap();
    assert_eq!(after.iter().filter(|s| s.kind == "recipient").count(), 1);
    alice.vault_lock(&vid, "user").unwrap();
    assert_eq!(code(anna.vault_open_with_key(vol.to_str().unwrap(), pw("anna"))), "not_recipient", "revoked");
    let info = alice.vault_open_with_key(vol.to_str().unwrap(), pw("chiave")).unwrap();
    alice.vault_lock(&info.id, "user").unwrap();

    assert_eq!(alice.my_key_remove().unwrap().public, None);
    assert!(!file.exists());
}

#[test]
fn portable_keeps_the_key_in_memory_only() {
    let t = tempfile::tempdir().unwrap();
    let st = app(None);
    let k = st.my_key_create(pw("p"), Some("fast")).unwrap();
    assert!(!k.persisted && k.public.is_some());
    assert_eq!(st.my_key_get().unwrap(), k);
    assert_eq!(app(None).my_key_get().unwrap().public, None, "nothing written anywhere");
    let out = t.path().join("k.nvkey");
    st.my_key_export(out.to_str().unwrap()).unwrap();
    let again = app(None);
    assert_eq!(again.my_key_import(out.to_str().unwrap(), pw("p")).unwrap().public, k.public);
    assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 1, "only the exported file");
}
