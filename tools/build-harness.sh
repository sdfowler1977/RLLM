#!/usr/bin/env bash
# Builds the connected-check runner (tools/harness.rs).
# `harness sweep` is the sanctioned record-writing verb: its console summary and
# verification/RESULTS.json census are the measurement. The bare run and
# `harness <group>` do not write the record. Never run `harness sweep` from a
# lane: it rewrites the delivered record.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
output=${1:-"$project_dir/output/harness"}
mkdir -p "$(dirname "$output")"
printf 'harness compiler: %s\n' "$("$CLIENT_RUST/bin/rustc" --version)"
LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
  "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name harness \
  --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
  -C linker-features=-lld "$project_dir/tools/harness.rs" -o "$output"
printf 'runner: %s (run it from %s)\n' "$output" "$project_dir"
bash "$project_dir/tools/build-verification.sh" "$project_dir"
