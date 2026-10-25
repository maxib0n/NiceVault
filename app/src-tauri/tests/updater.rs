//! Updater: signatures, in-place replacement (file and .app bundle, with rollback), and an
//! end-to-end update of a dummy binary from a local HTTP server.

use base64::Engine;
use nicevault_app::updater::{
    self, InstallOutcome, Target, UpdateStatus, Updater, UpdaterConfig, replace_bundle, replace_file, sibling, swap_in,
    verify_signature,
};
use std::io::Write;
use std::path::{Path, PathBuf};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// A throwaway key pair in Tauri's format: (base64 public key, signer).
struct Key {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}
impl Key {
    fn new() -> Key {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Key { pk: kp.pk, sk: kp.sk }
    }
    fn pubkey_b64(&self) -> String {
        B64.encode(self.pk.to_box().unwrap().to_string())
    }
    /// What `tauri signer sign` writes to `<file>.sig` (and latest.json carries).
    fn sign_b64(&self, data: &[u8]) -> String {
        let sig = minisign::sign(Some(&self.pk), &self.sk, data, Some("file:test"), Some("signature from tauri secret key")).unwrap();
        B64.encode(sig.to_string())
    }
    /// What scripts/release.mjs (examples/sign_update.rs) writes: version + platforms bound.
    fn sign_release(&self, data: &[u8], version: &str, platforms: &str) -> String {
        let tc = format!("timestamp:1\tfile:NiceVault-linux-x86_64\tversion:{version}\tplatform:{platforms}");
        let sig = minisign::sign(Some(&self.pk), &self.sk, data, Some(&tc), Some("signature from tauri secret key")).unwrap();
        B64.encode(sig.to_string())
    }
}

#[test]
fn signature_good_bad_tampered() {
    let k = Key::new();
    let data = b"the new NiceVault".to_vec();
    let sig = k.sign_b64(&data);
    verify_signature(&k.pubkey_b64(), &data, &sig).unwrap();

    // tampered artefact
    let mut t = data.clone();
    t[0] ^= 1;
    assert_eq!(verify_signature(&k.pubkey_b64(), &t, &sig).unwrap_err().code, "update_signature");
    // signed by another key
    let other = Key::new();
    assert_eq!(verify_signature(&k.pubkey_b64(), &data, &other.sign_b64(&data)).unwrap_err().code, "update_signature");
    // garbage / empty signature
    assert_eq!(verify_signature(&k.pubkey_b64(), &data, "bm90IGEgc2lnbmF0dXJl").unwrap_err().code, "update_signature");
    assert_eq!(verify_signature(&k.pubkey_b64(), &data, "").unwrap_err().code, "update_signature");
    // tampered trusted comment inside the signature
    let text = String::from_utf8(B64.decode(&sig).unwrap()).unwrap().replace("file:test", "file:evil");
    assert_eq!(verify_signature(&k.pubkey_b64(), &data, &B64.encode(text)).unwrap_err().code, "update_signature");
}

#[test]
fn signature_from_tauri_signer_verifies() {
    // Produced by `tauri signer sign` (prehashed minisign) for the file content "hello\n" with a
    // throwaway key; checks the exact wire format the release script will produce.
    let pk = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDM1MzNDNzUzNzA2RTBBRjMKUldUekNtNXdVOGN6TlZLRzVPYkd6UHlWOWRTVmZVTENYTXRKc1Y0UUcvamdTSno4bHUxWkRnZEsK";
    let sig = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUekNtNXdVOGN6TlNwNmt5OHJPRXN3dlFFTzZaYWFBakVKRDdtL21vOHlpOG4wanI3QjBMMUhBSUUwclVuWlM4cFNSVk4yalZxRmNBYzhJNHZKWGdTajEwR3M1NVVXendnPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyODU3NTM4CWZpbGU6YS5iaW4KQTZPVHR0RHVGQlI2U1pjbStPeUdxZm5XNDFLTi9CRzdZNnp0R2tHUHZxRURxS2hHamJ6ZFllQVZSV3MxSlpSa0VjcFpmak9iSW82UnVxVy9qQU1JRHc9PQo=";
    verify_signature(pk, b"hello\n", sig).unwrap();
    assert!(verify_signature(pk, b"hello!\n", sig).is_err());
}

#[test]
fn replace_file_keeps_old_and_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("NiceVault-linux-x86_64");
    std::fs::write(&exe, b"v1").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    replace_file(&exe, b"v2").unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"v2");
    assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"v1");
    assert!(!sibling(&exe, ".new").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&exe).unwrap().permissions().mode() & 0o111, 0o111, "stays executable");
    }

    // a second update replaces the previous .old
    replace_file(&exe, b"v3").unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"v3");
    assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"v2");

    // the staged file vanishes before the final rename: the current exe must survive intact
    assert!(swap_in(&exe, &dir.path().join("missing.new")).is_err());
    assert_eq!(std::fs::read(&exe).unwrap(), b"v3");
}

#[test]
fn bundle_swap_rolls_back() {
    // Directories take the two-rename path: the bundle is moved to .old, then the (missing)
    // staged bundle fails to move in, and the original must be put back.
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("NiceVault.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    std::fs::write(bundle.join("Contents/MacOS/nicevault"), b"old").unwrap();
    assert!(swap_in(&bundle, &dir.path().join("nope.app")).is_err());
    assert_eq!(std::fs::read(bundle.join("Contents/MacOS/nicevault")).unwrap(), b"old");
    assert!(!sibling(&bundle, ".old").exists());
}

fn app_zip(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data, mode) in files {
        // deflated like `zip -r` in build-all.sh (the real bundle's binary and plist)
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(*mode);
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn replace_bundle_swaps_the_whole_app() {
    let dir = tempfile::tempdir().unwrap();
    // the user renamed the bundle: the name is kept
    let bundle = dir.path().join("Il mio NiceVault.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    std::fs::write(bundle.join("Contents/MacOS/nicevault"), b"old").unwrap();
    std::fs::write(bundle.join("Contents/stale-file"), b"x").unwrap();

    let zip = app_zip(&[
        ("NiceVault.app/Contents/Info.plist", b"<plist/>", 0o644),
        ("NiceVault.app/Contents/MacOS/nicevault", b"new", 0o755),
        ("NiceVault.app/Contents/_CodeSignature/CodeResources", b"sig", 0o644),
    ]);
    replace_bundle(&bundle, &zip).unwrap();
    assert_eq!(std::fs::read(bundle.join("Contents/MacOS/nicevault")).unwrap(), b"new");
    assert_eq!(std::fs::read(bundle.join("Contents/_CodeSignature/CodeResources")).unwrap(), b"sig");
    assert!(!bundle.join("Contents/stale-file").exists(), "the bundle is replaced, not merged");
    assert_eq!(std::fs::read(sibling(&bundle, ".old").join("Contents/MacOS/nicevault")).unwrap(), b"old");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(bundle.join("Contents/MacOS/nicevault")).unwrap().permissions().mode() & 0o777, 0o755);
    }

    // a zip that escapes the bundle is refused and nothing changes
    let evil = app_zip(&[("NiceVault.app/../../evil", b"x", 0o644)]);
    assert!(replace_bundle(&bundle, &evil).is_err());
    let not_app = app_zip(&[("Other/thing", b"x", 0o644)]);
    assert!(replace_bundle(&bundle, &not_app).is_err());
    assert_eq!(std::fs::read(bundle.join("Contents/MacOS/nicevault")).unwrap(), b"new");
    assert!(!sibling(&bundle, ".new").exists());
}

/// Chained symlinks. `Contents/l1 -> ..` points at the bundle root (one `..` for a
/// two-level path passed the old count check), `Contents/l1/l2 -> ..` is then created THROUGH l1
/// at the bundle root and points out of it, and `Contents/l1/l2/evil` lands next to the bundle.
/// Now: links may only point down, and nothing is written through a link.
#[cfg(unix)]
#[test]
fn bundle_symlinks_cannot_reach_outside() {
    fn zip_with(entries: &[(&str, Option<&str>)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, link) in entries {
            match link {
                Some(t) => w.add_symlink(*name, *t, opts).unwrap(),
                None => {
                    w.start_file(*name, opts).unwrap();
                    w.write_all(b"x").unwrap();
                }
            }
        }
        w.finish().unwrap().into_inner()
    }
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("stage").join("NiceVault.app");
    let chained = zip_with(&[
        ("NiceVault.app/Contents/l1", Some("..")),
        ("NiceVault.app/Contents/l1/l2", Some("..")),
        ("NiceVault.app/Contents/l1/l2/evil", None),
    ]);
    assert!(updater::extract_bundle(&chained, &dest).is_err());
    assert!(!dir.path().join("stage/evil").exists() && !dir.path().join("evil").exists(), "a file was written outside the bundle");
    // a file entry under a link to a folder inside the bundle is refused too (no writing through links)
    let through = zip_with(&[("NiceVault.app/Contents/A/x", None), ("NiceVault.app/Contents/Cur", Some("A")), ("NiceVault.app/Contents/Cur/y", None)]);
    assert!(updater::extract_bundle(&through, &dir.path().join("t.app")).is_err());
    // the macOS framework shape (links that point down) still extracts
    let ok = zip_with(&[
        ("NiceVault.app/Contents/Frameworks/F.framework/Versions/A/F", None),
        ("NiceVault.app/Contents/Frameworks/F.framework/Versions/Current", Some("A")),
        ("NiceVault.app/Contents/Frameworks/F.framework/F", Some("Versions/Current/F")),
    ]);
    let good = dir.path().join("good.app");
    updater::extract_bundle(&ok, &good).unwrap();
    assert_eq!(std::fs::read(good.join("Contents/Frameworks/F.framework/F")).unwrap(), b"x");
}

// ---------------------------------------------------------------------------------------------
// end to end

/// Serves the files `make(base_url)` returns on 127.0.0.1 until the test process ends.
fn serve(make: impl FnOnce(&str) -> Vec<(String, Vec<u8>)>) -> String {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let files = make(&url);
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let hit = files.iter().find(|(p, _)| p == req.url()).map(|(_, b)| b.clone());
            let _ = match hit {
                Some(b) => req.respond(tiny_http::Response::from_data(b)),
                None => req.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    url
}

fn manifest(version: &str, platform: &str, url: &str, sig: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version": version,
        "notes": "Faster unlock.",
        "notes_i18n": { "it": "Sblocco più veloce.", "en": "Faster unlock." },
        "pub_date": "2026-10-24T12:00:00Z",
        "platforms": { platform: { "signature": sig, "url": url } }
    }))
    .unwrap()
}

fn updater(url: &str, key: &Key, exe: &Path, config: Option<PathBuf>, downloads: &Path, in_place: bool) -> Updater {
    Updater::new(UpdaterConfig {
        manifest_url: format!("{url}/latest.json"),
        pubkey: key.pubkey_b64(),
        current_version: "0.1.0".into(),
        platform: updater::platform_key().into(),
        exe: exe.to_path_buf(),
        config_dir: config,
        downloads_dir: Some(downloads.to_path_buf()),
        in_place,
    })
}

#[test]
fn end_to_end_updates_a_dummy_binary() {
    let key = Key::new();
    let new_bin = b"#!/bin/sh\necho 0.2.0\n".to_vec();
    let plat = updater::platform_key();
    let sig = key.sign_release(&new_bin, "0.2.0", &format!("other,{plat}"));
    let srv_url = serve(|url| {
        vec![
            ("/latest.json".into(), manifest("0.2.0", plat, &format!("{url}/NiceVault-linux-x86_64"), &sig)),
            ("/NiceVault-linux-x86_64".into(), new_bin.clone()),
        ]
    });
    let stick = tempfile::tempdir().unwrap();
    let exe = stick.path().join("NiceVault-linux-x86_64");
    std::fs::write(&exe, b"old build").unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    let u = updater(&srv_url, &key, &exe, Some(config.path().to_path_buf()), downloads.path(), true);
    match u.check(false, 1_000).unwrap() {
        UpdateStatus::Available { version, current, notes, .. } => {
            assert_eq!((version.as_str(), current.as_str()), ("0.2.0", "0.1.0"));
            assert_eq!(notes.it, "Sblocco più veloce.");
        }
        s => panic!("{s:?}"),
    }
    let mut seen = vec![];
    u.download(&mut |d, t| seen.push((d, t))).unwrap();
    assert_eq!(seen.last().unwrap().0, new_bin.len() as u64);
    match u.install().unwrap() {
        InstallOutcome::Replaced { path } => assert_eq!(Path::new(&path), exe),
        o => panic!("{o:?}"),
    }
    assert_eq!(std::fs::read(&exe).unwrap(), new_bin);
    assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"old build");
    #[cfg(unix)]
    {
        let out = std::process::Command::new(&exe).output().expect("the updated file runs");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "0.2.0");
    }

    // installed mode: last check persisted; the automatic check is then skipped for a day
    let again = updater(&srv_url, &key, &exe, Some(config.path().to_path_buf()), downloads.path(), true);
    assert_eq!(again.prefs().last_check, Some(1_000));
    assert_eq!(again.check(true, 1_000 + 3_600_000).unwrap(), UpdateStatus::Skipped);
    assert!(matches!(again.check(true, 1_000 + 25 * 3_600_000).unwrap(), UpdateStatus::Available { .. }));
    again.set_check_on_start(false);
    assert_eq!(again.check(true, u64::MAX).unwrap(), UpdateStatus::Skipped);
    assert!(!updater(&srv_url, &key, &exe, Some(config.path().to_path_buf()), downloads.path(), true).prefs().check_on_start);

    // portable: nothing persisted
    let p = updater(&srv_url, &key, &exe, None, downloads.path(), true);
    p.check(false, 5).unwrap();
    assert!(p.prefs().portable);
    assert!(!stick.path().join("updater.json").exists());
    assert_eq!(std::fs::read_dir(stick.path()).unwrap().count(), 2, "only the exe and its .old on the stick");

    // not in place (installed system-wide / dev build): verified file goes to Downloads
    let d = updater(&srv_url, &key, &exe, None, downloads.path(), false);
    d.check(false, 5).unwrap();
    d.download(&mut |_, _| {}).unwrap();
    match d.install().unwrap() {
        InstallOutcome::Downloaded { path } => {
            assert_eq!(Path::new(&path), downloads.path().join("NiceVault-linux-x86_64-0.2.0"));
            assert_eq!(std::fs::read(&path).unwrap(), new_bin);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn end_to_end_refuses_tampered_artifact_and_up_to_date() {
    let key = Key::new();
    let plat = updater::platform_key();
    let good = b"genuine".to_vec();
    let url = serve(|url| {
        vec![
            ("/latest.json".into(), manifest("0.2.0", plat, &format!("{url}/bin"), &key.sign_release(&good, "0.2.0", plat))),
            ("/bin".into(), b"tampered".to_vec()),
            ("/same/latest.json".into(), manifest("0.1.0", plat, &format!("{url}/bin"), "x")),
            ("/evil/latest.json".into(), manifest("0.2.0", plat, "http://10.0.0.1/bin", "x")),
        ]
    });
    let stick = tempfile::tempdir().unwrap();
    let exe = stick.path().join("nv");
    std::fs::write(&exe, b"old").unwrap();

    let u = updater(&url, &key, &exe, None, stick.path(), true);
    assert!(matches!(u.check(false, 0).unwrap(), UpdateStatus::Available { .. }));
    assert_eq!(u.download(&mut |_, _| {}).unwrap_err().code, "update_signature");
    assert_eq!(u.install().unwrap_err().code, "update_none");
    assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(stick.path()).unwrap().count(), 1);

    let same = updater(&format!("{url}/same"), &key, &exe, None, stick.path(), true);
    assert_eq!(same.check(false, 0).unwrap(), UpdateStatus::UpToDate { current: "0.1.0".into() });

    let evil = updater(&format!("{url}/evil"), &key, &exe, None, stick.path(), true);
    assert_eq!(evil.check(false, 0).unwrap_err().code, "update_invalid");

    let offline = updater("http://127.0.0.1:9", &key, &exe, None, stick.path(), true);
    assert_eq!(offline.check(false, 0).unwrap_err().code, "update_network");
}

/// The file latest.json is unsigned, so whoever serves it could offer an older genuine build as
/// "9.9.9" (rollback) or another platform's build. The signed trusted comment must match.
#[test]
fn end_to_end_refuses_rollback_and_wrong_platform() {
    let key = Key::new();
    let plat = updater::platform_key();
    let old = b"genuine 0.0.9".to_vec();
    let url = serve(|url| {
        let m = |v: &str, sig: String| manifest(v, plat, &format!("{url}/bin"), &sig);
        vec![
            ("/rollback/latest.json".into(), m("9.9.9", key.sign_release(&old, "0.0.9", plat))),
            ("/same/latest.json".into(), m("9.9.9", key.sign_release(&old, "0.1.0", plat))),
            ("/platform/latest.json".into(), m("0.2.0", key.sign_release(&old, "0.2.0", "plan9-mips"))),
            ("/unbound/latest.json".into(), m("0.2.0", key.sign_b64(&old))),
            ("/ok/latest.json".into(), m("0.2.0", key.sign_release(&old, "0.2.0", plat))),
            ("/bin".into(), old.clone()),
        ]
    });
    let stick = tempfile::tempdir().unwrap();
    let exe = stick.path().join("nv");
    std::fs::write(&exe, b"old").unwrap();
    for case in ["rollback", "same", "platform", "unbound"] {
        let u = updater(&format!("{url}/{case}"), &key, &exe, None, stick.path(), true);
        assert!(matches!(u.check(false, 0).unwrap(), UpdateStatus::Available { .. }), "{case}");
        assert_eq!(u.download(&mut |_, _| {}).unwrap_err().code, "update_signature", "{case}");
        assert_eq!(u.install().unwrap_err().code, "update_none", "{case}");
    }
    let ok = updater(&format!("{url}/ok"), &key, &exe, None, stick.path(), true);
    ok.check(false, 0).unwrap();
    ok.download(&mut |_, _| {}).unwrap();
    assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    assert!(updater::check_binding("timestamp:1\tfile:x\tversion:0.2.0\tplatform:a,b", "0.2.0", "0.1.0", "b").is_ok());
}

#[test]
fn target_writability() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Target::File(dir.path().join("x")).writable());
}

/// Running from an AppImage: the manifest key is `linux-x86_64-appimage` (never the bare
/// binary's `linux-x86_64`, which would replace the AppImage with something that needs the
/// system's WebKitGTK), and the file replaced is the .AppImage itself.
#[test]
fn appimage_updates_from_its_own_manifest_key() {
    let key = Key::new();
    let image = b"appimage 0.2.0".to_vec();
    let bare = b"bare binary 0.2.0".to_vec();
    let sig_image = key.sign_release(&image, "0.2.0", "linux-x86_64-appimage");
    let sig_bare = key.sign_release(&bare, "0.2.0", "linux-x86_64");
    let url = serve(|url| {
        let both = serde_json::json!({
            "version": "0.2.0",
            "platforms": {
                "linux-x86_64-appimage": { "signature": sig_image, "url": format!("{url}/NiceVault-linux-x86_64.AppImage") },
                "linux-x86_64": { "signature": sig_bare, "url": format!("{url}/NiceVault-linux-x86_64") },
            }
        });
        let bare_only = manifest("0.2.0", "linux-x86_64", &format!("{url}/NiceVault-linux-x86_64"), &sig_bare);
        vec![
            ("/both/latest.json".into(), serde_json::to_vec(&both).unwrap()),
            ("/bare/latest.json".into(), bare_only),
            ("/NiceVault-linux-x86_64.AppImage".into(), image.clone()),
            ("/NiceVault-linux-x86_64".into(), bare.clone()),
        ]
    });
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("NiceVault-linux-x86_64.AppImage");
    std::fs::write(&exe, b"old appimage").unwrap();
    let make = |case: &str| {
        Updater::new(UpdaterConfig {
            manifest_url: format!("{url}/{case}/latest.json"),
            pubkey: key.pubkey_b64(),
            current_version: "0.1.0".into(),
            platform: "linux-x86_64-appimage".into(),
            exe: exe.clone(),
            config_dir: None,
            downloads_dir: Some(dir.path().to_path_buf()),
            in_place: true,
        })
    };
    // a manifest with only the bare binary offers an AppImage nothing
    assert!(matches!(make("bare").check(false, 0).unwrap(), UpdateStatus::UpToDate { .. }));
    let u = make("both");
    assert!(matches!(u.check(false, 0).unwrap(), UpdateStatus::Available { .. }));
    u.download(&mut |_, _| {}).unwrap();
    assert!(matches!(u.install().unwrap(), InstallOutcome::Replaced { .. }));
    assert_eq!(std::fs::read(&exe).unwrap(), image);
    assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"old appimage");
}
