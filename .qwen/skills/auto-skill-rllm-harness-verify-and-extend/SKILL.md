---
name: rllm-harness-verify-and-extend
description: Measure, extend, and honestly report the RLLM harness (pure-std Rust, no Cargo) — sanctioned runners, verification-group census, desktop host effect arms, and Android packaging gates
source: auto-skill
extracted_at: '2026-11-10T16:09:58.243Z'
---

# RLLM harness: measure → extend → report

RLLM is pure-std Rust with **no Cargo and no `cargo test`**. A raw `rustc --test` over the tree is not a
measurement; only these two instruments count, and every number you report must come from one of them
with the runner named.

## 1. Measure before touching anything

```bash
# unit suite (sanctioned). Build it with the named builder — do not hand-roll a rustc line.
bash tools/build-units.sh output/units
output/tools/gate output/units      # expect: "Rust cases passed: 517/517"
# group sweep
bash -c 'set -euo pipefail; : "${CLIENT_RUST:?}"; bash tools/build-harness.sh 2>/dev/null || \
  LD_LIBRARY_PATH="$CLIENT_RUST/lib" "$CLIENT_RUST/bin/rustc" --edition=2021 \
  --crate-name harness "$PWD/tools/harness.rs" -o output/harness'
output/harness sweep > output/checks-sweep.log 2>&1; echo SWEEP_EXIT=$?
grep -E '^summary:' output/checks-sweep.log            # measured in a full-input room at 08c39ea:
  # "summary: 64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks". The committed record in
  # this worktree prints 67 passed / 4 unavailable / 4215 checks because it holds the reference builds a
  # git-archive room lacks. Quote the number of the tree you actually ran.
grep -c CENSUS output/checks-sweep.log                 # must be 0
```

`tools/harness.rs` is **census-enforced**: `GROUPS ∪ HELPERS(transport) ∪ OPTIONAL(live_web)` must equal
the contents of `verification/`. A mismatch prints `CENSUS: …` lines and invalidates the sweep. Separately,
`INVENTORY DRIFT: the delivered report counts N groups while the runner has M` compares the runner with the
record read **before** the run, so the run that grows the inventory prints it exactly once and the next run
is clean — that line alone is not a product red. `output/checks/<name>.json` +
`output/checks/harness-ledger.json` are the per-group evidence. `harness sweep` is the record-writing
verb: `output/harness sweep` rewrites `verification/RESULTS.json`. The bare run (`output/harness`) and
`harness <group>` (`output/harness turn_background`) do not write the record.

## 2. Adding a verification group (three steps, never two)

1. Write `verification/<name>.py`: `check(name, value)` asserts and appends, then write
   `output/checks/<name>.json` with `{passed, checks, scope}` and print `{count, scope}`.
2. Add the name to `GROUPS` in `tools/harness.rs`.
3. **Rebuild `output/harness`.** A stale binary silently keeps the old group count — the sweep then
   certifies nothing. Re-run the sweep and confirm the summary's `of N groups` moved and CENSUS is 0.

Make the group *discriminating*, not decorative: assert the negative arms too (foreign key, truncated
vault, malformed request) and parse the subject's own output shape exactly (e.g. the host's leg lines are
`"ok   "` — five characters; slicing at 4 leaves a leading space and every name check fails).

## 3. Extending the Linux/Windows host (`core/desktop.rs`)

Build entry is `desktop.sh` (crate root `core/desktop.rs`, which `include!("lib.rs")`); `console.rs` is the
headless bin. The host is: startup render → stdin action loop → `advance(state, root, action, inputs)` with
`EFFECT_HOPS = 32`. An effect kind with no arm becomes a scoped `failure`, so a missing host capability
looks like a model error.

Effect contract you must honor when adding an arm:

- `network`: effect carries `host/port/tls/purpose` and **either** `wire` (text) **or** `wire_base64`
  (bytes). If a request stores a file, also `media_upload` + `upload_bytes` — stream it with
  `op:"media_upload_read"` `{ticket,purpose}` → `{data:b64,done}` or the round trip stalls mid-body.
- `tool`: `op:"tool"`, then action `tool_result`.
- `voice_listen`: send action `voice_audio_start`, then `op:"voice_audio_feed"` with **≤ 640-byte frames**
  (20 ms @ 16 kHz s16le), stop at `voice_lease_ms`, and on `{ready:true,receipt}` send action
  `voice_audio_result`. Read the mic in blocks **larger** than the frame and carry the remainder
  (`self.pending = bytes.split_off(want)`) — truncating per frame silently discards audio.
- `other` → scoped `failure` naming the missing capability.

Add the new leg to `self_test()` (its count line is asserted by `verification/desktop_runtime.py`, so the
self-test count and the named legs must agree) and to that group's required-leg list.

`type Result<T> = Result<T, String>`, so io errors need `.map_err(|e| e.to_string())`.

## 4. Android packaging gates (before blaming the product)

- `package.sh` runs `aapt2 compile --dir res`, so **nothing non-resource may live under `res/`** or the
  APK cannot be built at all.
- `client/Usage.java` needs **platform-35** (`UsageEventsQuery`) and rust-std for **both**
  `aarch64-` and `x86_64-linux-android`.
- No keystore here and the signing cert digest is pinned per variant → use `CLIENT_UNSIGNED=1` as a
  pipeline proof, and say so.
- `verification/apk_alignment.py` runs against `RLLM_CHECK_APK=<path>`; point it at a freshly built APK
  whose sources match the tree.
- The reference-build groups need `output/{previous,pre-branch,pre-auth,published}-controller`; with no
  git history they are **unavailable**, not failures. Report them in a table.
- Map each red row to the binary it actually opened before calling it a gap — the six non-green groups are
  only **three** missing files, read straight out of the group logs (`tail output/checks/<name>.log` ends in
  `FileNotFoundError: … '/home/…/output/<controller>'`): `previous-controller` → history_migration +
  prompt_reference; `pre-branch-controller` → binary_flow; `published-controller` → field_flow +
  flags_flow + product_flow. `output/pre-auth-controller` may legitimately stay absent (the three retained
  revision-two compatibility checks). A fifth input, `output/checks/published-comparator.json`, has **no
  producer anywhere** in `tools/`, `*.sh` or `verification/` — it can only be owner-supplied.

## 5. Reporting rules this user enforces

State the runner and exact tallies — as measured in this window: `851/851` core cases plus `57` isolated
application cases from `output/tools/gate <binary>` in a room of `8367d80`, and
"64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks" for the full-input room sweep —
separate genuine environmental blockers from harness gaps, keep `docs/AUDIT.md` carrying the dated
findings, and never describe a mockup as live state. A tally copied from an older commit is a claim, not
a measurement; re-measure or label the tree it came from.
