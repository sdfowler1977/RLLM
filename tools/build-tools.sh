#!/usr/bin/env bash
# Builds the native packaging tools that replace tools/alignment.py,
# tools/readback.py, tools/checkpoint.py, tools/signature.py, tools/apk.py
# and tools/toolchain.py. tools/package.rs is the native publisher.
# tools/package.py remains until a signed candidate is published with it.
# No Cargo and no Python.
# Each tool is built twice: the application binary, then a --test build of the
# same source that is executed here so the unit cases the source carries are
# actually run by a sanctioned instrument. The executed case count is printed
# per tool and must equal the number of #[test] cases in that source; any run
# failure, an unreadable count or a count mismatch exits non-zero.
set -euo pipefail
project_dir=$(cd "$(dirname "$0")/.." && pwd)
: "${CLIENT_RUST:?Set CLIENT_RUST to the Rust toolchain directory}"
output=${1:-"$project_dir/output/tools"}
mkdir -p "$output"
printf 'tools compiler: %s\n' "$("$CLIENT_RUST/bin/rustc" --version)"
for tool in alignment readback checkpoint signature apk toolchain gate package harness; do
  LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
    "$CLIENT_RUST/bin/rustc" --edition=2021 --crate-name "$tool" \
    --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
    -C linker-features=-lld "$project_dir/tools/$tool.rs" -o "$output/$tool"
  printf 'tool: %s\n' "$output/$tool"
  LD_LIBRARY_PATH="$CLIENT_RUST/lib:${LD_LIBRARY_PATH:-}" \
    "$CLIENT_RUST/bin/rustc" --test --edition=2021 --crate-name "$tool" \
    --remap-path-prefix "$project_dir=RLLM" -C opt-level=2 -C codegen-units=1 \
    -C linker-features=-lld "$project_dir/tools/$tool.rs" -o "$output/$tool-tests"
  if ! report=$("$output/$tool-tests" 2>&1); then
    printf 'tool tests: %s cases: none (test run failed)\n' "$output/$tool-tests" >&2
    printf '%s\n' "$report" >&2
    exit 1
  fi
  cases=$(printf '%s\n' "$report" |
    sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed; 0 failed;.*/\1/p')
  declared=$(grep -c '#\[test\]' "$project_dir/tools/$tool.rs" || true)
  if [[ ! "$cases" =~ ^[1-9][0-9]*$ || "$cases" != "$declared" ]]; then
    printf 'tool tests: %s cases: %s (source declares %s)\n' \
      "$output/$tool-tests" "${cases:-unreadable}" "$declared" >&2
    printf '%s\n' "$report" >&2
    exit 1
  fi
  printf 'tool tests: %s cases: %s\n' "$output/$tool-tests" "$cases"
done
bash "$project_dir/tools/build-verification.sh" "$project_dir"
