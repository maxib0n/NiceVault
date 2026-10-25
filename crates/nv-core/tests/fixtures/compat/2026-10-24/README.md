# Compatibility fixtures, set 2026-10-24

Real volumes written by commit `e6e385c` ("Backend defaults match the UI: accent velluto
(replaces bordeaux), theme dark") — exactly the build that was installed when the first
real volume was created. **Never edit, regenerate or delete these files.** Every later build
must open them and read back everything in `manifest.json` (tests `crates/nv-core/tests/compat.rs`
and `app/src-tauri/tests/compat.rs`; both work on temp copies and check the files are untouched).

Generated with `app/src-tauri/tests/compat_gen.rs` at that commit, only through its public APIs
(app layer `AppState` for create / settings / appearance, `nv_core::Vault` for files, slots and the
password manager):

    NV_COMPAT_OUT=crates/nv-core/tests/fixtures/compat/2026-10-24 NV_COMPAT_COMMIT=$(git rev-parse HEAD) \
      cargo test -p nicevault-app --test compat_gen -- --ignored --nocapture

KDF: every slot uses `KdfPreset::Fast` (Argon2id m = 8 MiB, t = 1, p = 1) for test speed, except
`nudo.nvault`, created with `KdfPreset::Strong` as calibrated on the generating machine
(m = 512 MiB, t = 1, p = 4 — the parameters real volumes use).

| file | password(s) | what it covers |
|---|---|---|
| `archivio.nvault` | slot 0 `archivio-principale` ("Principale"), slot 1 `riserva ✓ üñí 密码` ("Riserva — cassaforte 🔐"), slot 2 = hole (removed), slot 3 recipient `archivio.nvkey` (key-file password `chiave-identita`, "Chiave di casa"), slot 4 **duress** `archivio-distruggi` ("Emergenza") | files: empty, small, exactly 1 chunk, 3-chunk odd size, overwritten + truncated; nested folders, unicode / NFD / space / dot names; renamed+moved file; deleted file and folder; fixed mtimes. Image emblem visible (256 px inside, 128 px copy in the header), accent velluto, theme light, auto-lock 5 min, no lock on sleep, scope all. 4 password-manager entries (TOTP SHA1 and SHA256/8/60, hidden field, history from updates and from import), one deleted entry |
| `cifrato.nvault` | `cifrato-pw` ("Unica") | appearance **encrypted** (no header section; icon `key` + accent prussia inside only), theme system, auto-lock never, no lock on screen lock; PM entry with TOTP SHA512/7/45 |
| `bordeaux.nvault` | `bordeaux-pw` | simulated pre-velluto volume: settings JSON `{"auto_lock_minutes":30,"accent":"bordeaux"}` (no theme / lock fields) and header accent `bordeaux` (that build could no longer store it, so it was written with `Vault::meta_set` / `Vault::set_appearance`); must read as velluto, theme dark |
| `nudo.nvault` | `nudo-pw` | nv-core only: no app meta keys, no header appearance section, Strong KDF |

The duress password destroys the volume's keys when entered: the tests try it only on a copy.
All data is fake. `manifest.json` records, per volume: file sha256 / size, `peek` (header-only
view), slots (kind, label, KDF, fingerprint), every node (path, id, size, mtime, content sha256),
paths that must not exist, raw `meta` values, password-manager entries with all secrets, history
and a TOTP code at a fixed time, and what the app showed (Benvenuto entry, `VaultInfo`, appearance,
settings, header section after an app unlock).
