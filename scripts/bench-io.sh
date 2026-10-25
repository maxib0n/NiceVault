#!/usr/bin/env bash
# Write-path benchmark (see crates/nv-mount/examples/nv_bench.rs).
#   scripts/bench-io.sh VOL_DIR [SRC_DIR] [MODES] [WORKLOADS]
# VOL_DIR: where the volume (and plain copy) go — e.g. a folder on a USB stick.
# SRC_DIR: generated source files (default VOL_DIR/src; keep it on a fast local disk).
# MODES default "plain api fuse"; WORKLOADS default "2000x200000 50x20000000 2x1000000000".
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
vol=${1:?VOL_DIR}; src=${2:-$vol/src}
modes=${3:-plain api fuse}
loads=${4:-2000x200000 50x20000000 2x1000000000}
cargo build --release -q -p nv-mount --example nv_bench --manifest-path "$here/Cargo.toml"
bin=${CARGO_TARGET_DIR:-$here/target}/release/examples/nv_bench
for w in $loads; do
  for m in $modes; do
    "$bin" --vol-dir "$vol" --src-dir "$src" --mode "$m" --files "$w" --label "${LABEL:-}"
  done
done
