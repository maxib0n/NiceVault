//! Release signing (scripts/release.mjs): like `tauri signer sign`, but the trusted comment also
//! binds the version and the platforms, which the updater checks.
//!   cargo run -p nicevault-app --example sign_update -- <secret.key> <file> <version> <platform,...>
//! Writes `<file>.sig` (base64 of the minisign signature, the Tauri format).

use base64::Engine;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [key, file, version, platforms] = &a[..] else {
        eprintln!("usage: sign_update <secret.key> <file> <version> <platform,...>");
        std::process::exit(2)
    };
    let b64 = base64::engine::general_purpose::STANDARD;
    let sk_text = String::from_utf8(b64.decode(std::fs::read_to_string(key).expect("read key").trim()).expect("key: base64")).expect("key: utf-8");
    let pw = std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
    let sk = minisign::SecretKey::from_box(minisign::SecretKeyBox::from_string(&sk_text).expect("key box"), Some(pw)).expect("decrypt key");
    let name = std::path::Path::new(file).file_name().expect("file name").to_string_lossy();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let tc = format!("timestamp:{ts}\tfile:{name}\tversion:{version}\tplatform:{platforms}");
    let sig = minisign::sign(None, &sk, std::fs::File::open(file).expect("open file"), Some(&tc), Some("signature from tauri secret key"))
        .expect("sign");
    std::fs::write(format!("{file}.sig"), b64.encode(sig.to_string())).expect("write .sig");
}
