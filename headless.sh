#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")" && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
mkdir -p "$project_dir/output/headless"
LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
  "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name client --crate-type bin \
  --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
  -C linker-features=-lld "$project_dir/core/console.rs" -o "$project_dir/output/headless/RLLM"
printf '%s\n' "$project_dir/output/headless/RLLM"
