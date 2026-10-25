# nfsserve 0.11.0 (vendored, patched)

Unmodified copy of crates.io `nfsserve` 0.11.0 except `src/rpcwire.rs` (`MAX_RECORD`, the check
in `read_fragment`): RPC records over 2 MiB drop the connection before anything is allocated
(a NiceVault patch). Wired by
`[patch.crates-io]` in the workspace `Cargo.toml`. To update: copy the new upstream release here
and re-apply the `NiceVault patch` hunk (`grep -n "NiceVault patch" src/rpcwire.rs`).
