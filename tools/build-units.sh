#!/usr/bin/env bash
# Builds the unit-test binary that the sanctioned runner measures:
#   output/tools/gate output/units
# A bare `rustc --test` summary is not the measurement; the runner's
# "Rust cases passed: N/M; isolated application cases: K" line is.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
output=${1:-"$project_dir/output/units-pinned"}
mkdir -p "$(dirname "$output")"
printf 'units compiler: %s\n' "$("$CLIENT_RUST/bin/rustc" --version)"
LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
  "$CLIENT_RUST/bin/rustc" --test --edition=2021 --crate-name client \
  --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
  -C linker-features=-lld "$project_dir/core/console.rs" -o "$output"
printf 'unit binary: %s\n' "$output"
