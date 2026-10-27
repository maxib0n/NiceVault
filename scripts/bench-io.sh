#!/usr/bin/env bash
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
