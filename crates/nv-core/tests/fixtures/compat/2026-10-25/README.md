# Compatibility fixtures, set 2026-10-25 — single-file format (format v2 journal)

First set of **single-file volumes** (header `journal` section: the rollback journal lives inside
the `.nvault`). Written by the commit
that adds this folder (its parent is the `generated_by_commit` in `manifest.json`; the generator and
the core code it ran are the ones committed together with these files). **Never edit, regenerate or
delete these files.** Every later build must open them and read back everything in `manifest.json`
(`crates/nv-core/tests/compat.rs`, `app/src-tauri/tests/compat.rs`); builds before this format refuse
them with `unsupported_version` (they never write to them).

Generated with:

    NV_INTERRUPTED_OUT=/tmp/x/interrotto.nvault cargo test -p nv-core --lib single_tests::gen_interrupted_fixture -- --ignored
    NV_COMPAT_SINGLE=1 NV_INTERRUPTED=/tmp/x/interrotto.nvault \
      NV_COMPAT_OUT=crates/nv-core/tests/fixtures/compat/2026-10-25 NV_COMPAT_COMMIT=$(git rev-parse HEAD) \
      cargo test -p nicevault-app --test compat_gen -- --ignored --nocapture

| file | password | what it covers |
|---|---|---|
| `archivio.nvault` | as in set 2026-10-24 (`archivio-principale`, `riserva ✓ üñí 密码`, key file `archivio.nvkey` / `chiave-identita`, duress `archivio-distruggi`) | the full archivio scenario of 2026-10-24, made by this build, then **"Aggiorna formato"** (database < 4 MiB: moved after the region, `split = region`) |
| `cifrato.nvault` | `cifrato-pw` | encrypted appearance, then upgraded |
| `bordeaux.nvault` | `bordeaux-pw` | simulated pre-velluto settings, then upgraded |
| `nudo.nvault` | `nudo-pw` | nv-core only, Strong KDF, then upgraded |
| `grande.nvault` | `grande-pw` | database larger than the 4 MiB region: its first 4 MiB moved past the old end (`split > region`, three-part mapping) |
| `interrotto.nvault` | `interrotto-pw` | region 128 KiB; left by a crash mid-transaction with a **hot in-file journal** continuing in its tail past the database; must roll back to `tenuto.bin` + `piccolo.txt` (no `perso.bin`) |

KDF: Fast everywhere (Argon2id m = 8 MiB, t = 1, p = 1) except `nudo.nvault` (Strong). The files are
mostly zeros (the 4 MiB journal region), so they are small in git. All data is fake.
