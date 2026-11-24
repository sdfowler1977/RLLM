#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd "$(dirname "$0")" && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
export PATH="${HOME}/.cargo/bin:${CLIENT_RUST}/bin:${PATH:-}"
mkdir -p "$project_dir/output/desktop/linux/x86_64" "$project_dir/output/desktop/linux/aarch64"
rustc_bin="$CLIENT_RUST/bin/rustc"
host=$("$rustc_bin" -vV | awk '/^host: / { print $2 }')
if command -v rustup >/dev/null 2>&1; then
  rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
fi
build_target() {
  local arch=$1
  local target=$2
  local dest="$project_dir/output/desktop/linux/$arch/RLLM-Desktop"
  # Stable rejects linker-features on aarch64-unknown-linux-gnu, so that target
  # keeps the toolchain default. Other targets still opt out of the self-contained linker.
  if [[ "$target" == "aarch64-unknown-linux-gnu" ]]; then
    LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
      "$rustc_bin" --edition=2021 --crate-name client --crate-type bin \
      --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
      --target "$target" \
      "$project_dir/core/desktop.rs" -o "$dest"
  else
    LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
      "$rustc_bin" --edition=2021 --crate-name client --crate-type bin \
      --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
      -C linker-features=-lld --target "$target" \
      "$project_dir/core/desktop.rs" -o "$dest"
  fi
  printf '%s\n' "$dest"
  if [[ "$host" == "$target" ]]; then
    cp "$dest" "$project_dir/output/desktop/RLLM-Desktop"
    printf '%s\n' "$project_dir/output/desktop/RLLM-Desktop"
  fi
}
build_target x86_64 x86_64-unknown-linux-gnu
build_target aarch64 aarch64-unknown-linux-gnu
