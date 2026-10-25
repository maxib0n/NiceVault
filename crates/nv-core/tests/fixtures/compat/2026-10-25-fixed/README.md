# Compatibility fixtures, set 2026-10-25-fixed — fixed-size single-file volumes

First set of **fixed-size volumes** (header `fixed` section) and the first with fixed-size volumes
that are **single-file** (header `fixed` + `journal` sections: the journal region inside the
preallocated file). Written by the commit that adds this folder (its parent is the `generated_by_commit` in
`manifest.json`; the generator and the core code it ran are the ones committed together with these
files). **Never edit, regenerate or delete these files.** Every later build must open them and read
back everything in `manifest.json` (`crates/nv-core/tests/compat.rs`, `app/src-tauri/tests/compat.rs`).
Builds before this one refuse the single-file ones with `unsupported_version` without writing a
byte (checked with master 9937784, the 2026-10-25 single-file build, and 039a916); they open
`fisso-vecchio.nvault` as before.

Generated with:

    NV_INTERRUPTED_FIXED_OUT=/tmp/x/fisso-interrotto.nvault cargo test -p nv-core --lib single_tests::gen_interrupted_fixed_fixture -- --ignored
    NV_COMPAT_FIXED=1 NV_INTERRUPTED_FIXED=/tmp/x/fisso-interrotto.nvault \
      NV_COMPAT_OUT=crates/nv-core/tests/fixtures/compat/2026-10-25-fixed NV_COMPAT_COMMIT=$(git rev-parse HEAD) \
      cargo test -p nicevault-app --test compat_gen -- --ignored --nocapture

| file | password | what it covers |
|---|---|---|
| `fisso.nvault` | `fisso-pw` | 8 MiB, made through the app (`vault_create_sized`): single-file, region 1 MiB in place of the footer region; visible icon emblem, accent ottanio, settings, labelled slot, a PM entry, a folder |
| `fisso-vecchio.nvault` | `fisso-vecchio-pw` | 6 MiB with the **side-file journal** and the footer region, as earlier builds made fixed-size volumes (same code path); `compat.rs::fixtures_upgrade_to_single_file` upgrades it |
| `fisso-aggiornato.nvault` | `fisso-aggiornato-pw` | 8 MiB side-file volume **upgraded in place** ("Aggiorna formato"): database larger than the displaced part of the region (`split > region - 64 KiB`, three-part mapping) |
| `fisso-esteso.nvault` | `fisso-esteso-pw` | single-file, made at 6 MiB (region 512 KiB), **extended** to 10 MiB (journal records of the new capacity), written after |
| `fisso-interrotto.nvault` | `fisso-interrotto-pw` | 6 MiB, region 128 KiB, store almost up to the capacity; left by a crash mid-transaction with a **hot journal whose tail lies past the capacity** (the file is longer than 6 MiB): opening it rolls back (no `perso.bin`, no `pieno/liberato.bin`) and cuts the file back to 6 MiB |

KDF: Fast everywhere (Argon2id m = 8 MiB, t = 1, p = 1). The files are their capacity long and
mostly zeros (small in git). All data is fake.
