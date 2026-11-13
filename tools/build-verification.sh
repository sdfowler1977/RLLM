#!/usr/bin/env bash
# Builds each native verification twin (verification/<name>.rs) into
# <project_root>/output/verification/<name>. verification/twin/mod.rs is the
# shared module those twins import; it is not a group binary.
# No Cargo and no Python.
set -euo pipefail
project_dir=$(cd "${1:?project root}" && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
output="$project_dir/output/verification"
mkdir -p "$output"
shopt -s nullglob
# A binary with no remaining twin source would still be preferred by the runner.
for built in "$output"/*; do
  [[ -f "$built" ]] || continue
  [[ -f "$project_dir/verification/$(basename "$built").rs" ]] || rm -f "$built"
done
twins=("$project_dir"/verification/*.rs)
if [[ ${#twins[@]} -eq 0 ]]; then
  printf 'verification twins: none\n'
  exit 0
fi
printf 'verification compiler: %s\n' "$("$CLIENT_RUST/bin/rustc" --version)"
for source in "${twins[@]}"; do
  name=$(basename "$source" .rs)
  # The runner prefers a built twin, so a failed compile must not leave the
  # previous binary in place to be mistaken for a pass of the new source.
  rm -f "$output/$name"
  LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
    "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name "$name" \
    --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
    -C linker-features=-lld "$source" -o "$output/$name"
  printf 'twin: %s\n' "$output/$name"
done
