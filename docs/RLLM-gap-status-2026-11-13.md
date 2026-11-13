# RLLM gap status, measured on the committed bytes (2026-11-13)

**Against:** `docs/RLLM-recursion-gap-report.md` (2026-11-11). That report states its own evidence is
**static** — "Every finding comes from reading the current RLLM source tree, not from running it. The tree
has no git metadata." Several of its ranked gaps have moved since, and two of its "missing entirely" claims
are now contradicted by measurement. This document records what I ran today, on which instrument, and what I
did **not** measure.

**Instruments used today (nothing else counts here):** the sanctioned tool gate
(`bash tools/build-tools.sh output/tools` → nine suites, each built twice and executed, with an executed-case
count interlock against the `#[test]` count in that source), the verification runner
(`output/harness <group>`, which does **not** rewrite `verification/RESULTS.json`), `tools/build-verification.sh`
(twin builds + the 44 helper cases), and git-archive witness rooms of committed bytes. Rows marked
**NOT MEASURED** are claims I am leaving as claims; they are not green.

Commit under measurement: `a9e8f114` ("two python legs retired on byte-identical pairs, and the TLS peer that
had to pass OpenSSL first"). Gate on it: `GATE22-TOOLS-RC=0` with package 46 / harness 28 cases and
`twin helper tests … cases: 44`, `GATE22-VERIFY-RC=0`. Amended the same day on `d31ec6c` after reading
`docs/RRSI-LOOP-RUNLOG.md` and `docs/SELF-BUILD-RUNLOG.md`: the Gap 1 and Gap 2/3 rows below first said those
run logs were "not on file", which is wrong — they are on file at older commits, and the correct statement is
about which of them still describe these bytes.

## Gap 1 — rebuild and reload itself

| row | status | evidence |
|---|---|---|
| the capability is wired | PRESENT | `output/harness self_rebuild` → `PASS (exit 0, 1s, 9 checks)`; `core/self_build.rs` is 5,118 lines |
| the delivered group's run log on `d31ec6c` | PRESENT (measured today) | git-archive room `/tmp/room-gap1` of `d31ec6c` (no `.git`, only `history_migration.py` + `live_web.py` left in `verification/`), room's own `bash headless.sh` → `rc=0`, room-built tool suites, then `output/harness self_rebuild` → `PASS (exit 0, 1s, 9 checks)` with all nine check names captured, including `spawn records a successor pid that is a child of this product process`, `the next self-build reaps that successor and the pid is gone`, `an omitted job records the carried job as inherited`; transcript `/tmp/room-gap1-run.log` |
| the whole edit → build → gate → commit → re-exec chain on `d31ec6c` | NOT MEASURED | the last full-chain log is `docs/SELF-BUILD-RUNLOG.md` §"The whole chain in one job, on committed bytes" at `57b063f` (2026-11-12); `core/self_build.rs` has not moved since, but that log is not on these bytes |

## Gap 2 — the thirteen RRSI features

The report's five "thin" bullets are all superseded on these bytes: refinement accepts **one to three**
rounds (`core/improvement.rs:113-121`, with `three_refinement_rounds_are_accepted_and_four_are_refused`
unit-pinned at `:893`), the loop edits harness **components** rather than instructions only
(`core/rrsi.rs:25` `const COMPONENTS: &[&str] = &["prompt", "control flow", "config", "context management",
"tools", "skills", "memory", "subagents"]`), and scoring/acceptance is no longer raw check counts.

| # | feature | status | measured evidence |
|---|---|---|---|
| 1 | multi-round harness editing, weights frozen | PRESENT | `output/harness rrsi_loop` → `PASS (exit 0, 1s, 15 checks)`; its log prints `three refinement rounds record budgets 4, 3 and 2, a leakage refusal, noise floor 2, pruned skills, the evolve commit, and held-out keys` |
| 2 | open edit space | PRESENT | `COMPONENTS` above, plus `core/rrsi.rs:37` `const EDITABLE` for the Harness² eight |
| 3 | annealed edit budget | PRESENT | `BUDGET_MAX 4` / `BUDGET_MIN 1` (`:117-118`), `annealed_budget` (`:771`); the group asserts 4→3→2 and, on resume, 4/4, 3/3, 3/1 |
| 4 | evidence-aware edit history | PRESENT | sealed chained JSONL (`seal` `:447`, `parse_chain` `:418`), `prior_rejection` (`:817`); group asserts `a re-proposed skills hypothesis is refused by name for round 1` |
| 5 | stall exploration reserves untried components | PRESENT | `STALL_WINDOW 3` (`:57`), `progress_stalled` (`:965`), `reserved_slot_refusal` (`:980`); group asserts `a stalled ledger refuses an already-tried component and does not refuse one that was never tried` |
| 6 | leakage critic before scoring | PRESENT | `LEAKAGE` kinds (`:47`), `annotate(edit, "clean")` at `core/harness.rs:126`; group asserts a leakage refusal **before** scoring |
| 7 | noise-adjusted floor | PRESENT | `recorded_noise_floor` (`:992`), `gain_inside_band` (`:952`); group records noise floor 2 in the three-round run and noise floor 0 in the flat-round arm |
| 8 | cost-aware acceptance | PARTIAL | `incumbent_cost` (`:915`), `cost_change` in `EDIT_KEYS`; group asserts `a flat memory shape is accepted for structural novelty, a shorter equal score is accepted for lower cost, and the same shape is refused when the cost rose`. The literal `ΔC ≤ β0 + β1·ΔS` form was **NOT MEASURED** |
| 9 | structural pruning | PRESENT | `PRUNE_WINDOW 3` (`:50`), `pruned_components` (`:885`); group asserts `pruned skills` and, as the negative arm, `two flat skills rounds leave the budget line unpruned` |
| 10 | evolve / held-out / OOD split | PARTIAL | `SPLIT_KEYS` (`:73`), `known_leg` (`:283`) with `held_out_known/passed/task_key/verifier`, `validate_split` (`:313`); group asserts the four held-out keys and `the separate evaluation score 0 differs from the in-round score 1 and is not a held-out key`. An **OOD** scoring pass (a different benchmark) was **NOT MEASURED** |
| 11 | parallel candidates; incumbent kept when none admissible | NOT MEASURED | `CANDIDATE_MAX 3` (`:101`), `WORKTREE_MAX 3`, `pool_status` (`:1561`). Route measured today: a refine reply that starts `candidates:` is parsed by `parse_pool` and handed to `evaluate_pool` at `core/harness.rs:710-711`; that function refuses a round carrying more than `CANDIDATE_MAX` (`:856-858`), refuses a probe/candidate count mismatch (`:859`), and on the recorded-worktree failure path stamps every pool row `admissible: false` and sets `incumbent_kept: true` with `round_note: "incumbent kept because no candidate was admissible"` (`:867-875`). The only group row about the pool is the **negative** one — `a non-refine run carries no workspace and does not attempt the pool` (`verification/rrsi_loop.rs:109`, asserting `!pool_text.contains("pool_status")`). No group ever drives a k-candidate pool to a decision. **Measured today through the product route** (a `refine` reply `candidates: 3\n{"/a":""}\n---\nCopy the task … into the harness\n---\ncomponent: skills …`, ledger `output/rrsi-loop-2397522-*/xdg/rllm/732a846e….jsonl`, 6 rows): the pool wrote **three rows in round 1** — `pool_status: admissible` (accepted, `score_before 0 → score_after 1`, its own `commit 41682a1c…`), `pool_status: refused:refused because the candidate contains a recorded task` (the leaking candidate, **no** score fields, `commit 69e4d5c6…`), and `pool_status: refused:the score did not rise` (`0 → 0`, `commit 97289e00…`). So a k-pool is judged, each candidate stamped with its own decision and commit, and one is admitted; the refusal of the whole pool to a "no candidate admissible" outcome, and the pool rows' admissibility as a **check target**, remain un-witnessed — today only the negative sentence `a non-refine run carries no workspace and does not attempt the pool` touches `pool_status` at all |
| 12 | git worktree candidates, incumbent is a commit | PRESENT | `require_recorded_worktree` on the `refine` path (`core/harness.rs:480`), `EVOLVE_BRANCH "rllm-evolve"` (`:103`), commit-in-slot (`:1997`); group asserts `the incumbent commit in .rllm-worktrees is the evolve tip` |
| 13 | auditable, resumable run | PARTIAL | JSONL ledger + chain, `STOP_FILE`/`STOP_SENTENCE`/`STOP_SYMLINK` (`:106-111`), `resume_run` (`:1665`); group asserts `files alone rebuild the resumed phase and decision` and `a tampered library entry is refused and its bytes stay unchanged`. Proposer/analyst/critic prompts **as plain files** was **NOT MEASURED** |

**Gap 2 proof owed — corrected after reading `docs/RRSI-LOOP-RUNLOG.md`.** That committed document already
carries the six-bar run log the report asks for — budgets 4/3/2 quoted out of the model request wire, the
leaking candidate refused *before* scoring (row 1, hypothesis stored as a digest), the noise-floor decision
(gain 1 inside floor 2), `pruned_components=["skills"]`, the incumbent as worktree commit
`e28d8f177ed8c616df9c23003af4df7d2b174a72` with `main` unmoved, and the four held-out keys with their digests
recomputed by the driver — all with values printed, made in a `git archive` room at commit `b8b0e71`
(2026-11-12). What is owed is narrower than "no artifact on file": that log is **stale for these bytes** —
`core/harness.rs` moved 45 lines and `core/rrsi.rs` 2 lines since `b8b0e71`, and the change is in the cost arm
(`acceptance_policy` gained a `measure_cost` parameter, and a dropped-field tie now leaves the cost unmeasured),
so the cost row of that log is no longer a witness. `rrsi_loop` now asserts all six bars as checks, but its log
names the values instead of printing them. **That leg is now added on this lane**: the group prints 27 value
lines (each budget sentence quoted out of the request wire it came from and labelled with that request, every
ledger row's noise floor for all three runs, the pruned set and the flat-run's absence, the incumbent commit as
ledger row / `refs/heads/rllm-evolve` / worktree HEAD, the held-out task and verifier digests, the sealed
library row next to the empty library) and its last check fails when a print degrades to `absent`.

## Gap 3 — Harness² two-level recursion

| # | item | status | measured evidence |
|---|---|---|---|
| 1 | task-level contrastive PPC | NOT MEASURED | the propose→probe→compose route exists in `core/harness.rs`; I did not witness a contrast-composed successor |
| 4 | **a later task reads the library the earlier task wrote** | PRESENT (product) as of this lane's group assertion; the earlier run log is stale for these bytes | witnessed at product level in `docs/RRSI-LOOP-RUNLOG.md` §Gap 3 at `b8b0e71`: two separate `output/headless/RLLM` processes, the second one's propose wire (9,799 bytes) carrying task t's `key 27c0edc8…`, `revision 665268c8…` and `harness_digest 65da7eef…` under `HISTORICAL EXPERIENCE`, and a third run under a different key shown the same entry as `indexed key is from another task`. Measured gap that this lane closes: no delivered group asserted it — `grep -rl "experience.jsonl|library_index|lesson_context" verification/` hit only sealing and tamper arms. `rrsi_loop` now carries `a later refine task reads the earlier task's sealed experience entry from the propose prompt` and `a refine task whose library holds no sealed row shows no historical experience section` |
| 5 | eight editable components | PRESENT | `core/rrsi.rs:37` `const EDITABLE = ["system prompt", "guardrails", "memory", "tool guidance", "subagents", "skills", "scripts", "plugins"]` |
| 2, 3, 6, 7, 8 | parallel invariant+domain candidates, sequential recursion, invariant/domain scoping, agentic selector over anonymised deliverables, resume with deliverables and reflection notes | **the deliverable / scoping layer is ABSENT (measured absence, not "unwitnessed"); resume is PRESENT and asserted** | The row vocabulary decides it. The keys the product seals are `EDIT_KEYS` = `component, hypothesis, diff_digest, score_before, score_after, cost_before, cost_after, verdict, round, run_digest` (`core/rrsi.rs:58-69`), `POLICY_KEYS` = `edit_budget, noise_floor, cost_known, decision` (`:70`), and the optional `SPLIT_KEYS` = `pool_status, branch, commit, held_out_known, ood_known, evolve_passed, held_out_passed, ood_passed, evolve_task_key, held_out_task_key, ood_task_key, evolve_verifier, held_out_verifier, ood_verifier, harness_digest, evolve_shape, ood_shape, beta0, beta1, accept_reason` (`:73-91`). None of them carries a deliverable, reflection, invariant or domain-scope field. Lexically the same way: `grep -ril deliverable core/ verification/` → **0 files**, likewise `reflection`, `invariant`, `domain_scop`; the single `anonym` hit is `core/search.rs:243`, an attribution sentence about a search API. Resume is the implemented half — `resume_run` called at `core/harness.rs:283-285`, with `STOP_FILE`/`STOP_SENTENCE`/`STOP_SYMLINK` — asserted by the group as `files alone rebuild the resumed phase and decision`, but what is rebuilt is a phase and a decision, not deliverables and reflection notes. The k-candidate pool exists (`CANDIDATE_MAX 3`, per-row `pool_status`) with **no** invariant/domain roles on its rows |

## Gap 4 — REA as a core tool

Two of the report's "missing entirely" rows are **present** on these bytes: `core/js_evidence.rs` (2,532
lines), `core/dotnet_evidence.rs` (3,428), `core/har_evidence.rs` (1,808).

| # | item | status | measured evidence |
|---|---|---|---|
| 1 | white-room feature rebuild, end to end | PRESENT | `output/harness pe_declarations` → `PASS (exit 0, 2s, 43 checks)`; it asserts `cited clean-room behavioural specification`, `clean-room artifact provenance names the behaviour document digest`, `clean-room implementation generated from cited behavioural claims`; `core/behaviour_spec.rs` emits `{"type":"clean_room_behaviour"}` documents sealed by `behaviour_digest` with claims **and** unknowns |
| 2 | decompiler-grade pseudocode | **ABSENT as pseudocode; the instruction IR underneath it is PRESENT and partly uncertified** | The word is absent: `grep -rl "pseudocode" core/` → no file. What the report did not credit: `core/instruction.rs` (536 lines) decodes AArch64 words (`decode` `:39`, `mnemonic` `:27`) and `analyze` (`:329`) emits a report headed `AArch64 instruction IR, candidate blocks and local register data flow`, one `Statement{offset,address,word,block,mnemonic,text,path_facts}` per instruction (`:25`) with `path_facts` filled by `conditional_paths`, refusing `Instruction analysis exceeds512 captured words`. It is wired: `core/control_flow.rs:290` `let ir=super::instruction::observe(&decoded,label,cancelled)?;` then `out.push_str(&ir.text)`. Group-level witness covers instruction facts, not the IR text: the registered group `binary_flow` (`tools/harness.rs:19`, 2,659 lines) assembles its subjects with `aarch64-linux-android26-clang` (`:315`) and asserts `CBZ target and register retained` and `legacy unclassified instruction remains historical evidence`, while `grep -rn "instruction IR\|candidate blocks\|data flow" verification/*.rs` → **0 hits**. Genuinely missing: *structured* pseudocode — control flow rebuilt as assignment / if / loop / return form with named temporaries; no emitter for that exists in `core/`. `core/instruction.rs` carries 29 `#[test]` cases and `core/control_flow.rs` 14, inside the pinned 878-case core-unit suite |
| 3 | JavaScript / Electron | PRESENT, wired, unit-witnessed; **no runner group names it** | `core/js_evidence.rs` (2,532 lines) is called from the product's clean-room route — `core/clean_room.rs:109-112` classifies a subject as `Asar` (refused: *"An asar container is not a feature source. No listing was generated."*), `Package` or `Script`, and `:195-196` parses the observation — and from `core/file_evidence.rs:87-91` (`javascript_observation`). Witnessed today by the pinned core unit suite (`bash tools/build-verification.sh`/`tools/build-units.sh` + `output/tools/gate output/units` → **`Rust cases passed: 878/878; isolated application cases: 57`**): `javascript_reference_items_match_the_regenerated_file` (`core/clean_room.rs:545`), `package_facts_match_the_regenerated_file` (`:666`), `asar_container_is_refused_by_name` (`:679`). No group in `const GROUPS` (71 names) drives a JS subject through the product route |
| 4 | .NET metadata / CIL | PRESENT, wired, unit-witnessed; **no runner group names it** | `core/dotnet_evidence.rs` (3,428 lines), entry `pub(super) fn cli_evidence` (`:2393`), called on the investigation route at `core/investigation.rs:131-136` (`Observed`/`Bounded`/`Absent`). Unit-witnessed by name in the same 878-case suite: `native_pe_without_cli_header_is_not_a_subject` (`:2571`), `cli_rva_outside_every_section_is_refused` (`:2584`), `pe_without_cli_directory_yields_no_dotnet_text` (`:2788`), `cli_directory_yields_exact_metadata_lines` (`:2799`), `inconsistent_cli_extent_is_refused_by_name` (`:2816`). Nothing at the runner-group level drives a .NET subject |
| 6 | saved HAR/SAZ captures | PRESENT | `pe_declarations` asserts `saved-capture rows read from workspace har and saz files` plus member/section/file-size boundaries |
| 7 | evidence contract (known vs unknown) | PRESENT | the behaviour document carries `claims` and `unknown` arrays, with `Every retained declaration row has an individual behavioural claim.` + a counted unknown when rows are skipped |
| 5, 8, 9 | web surface, desktop/package survey, firmware/packet/APK breadth | NOT MEASURED | — |

## Gap 5 — python in RLLM's own tooling

Measured on `a9e8f114`:

| row | measurement |
|---|---|
| python spawned by native code | **1** site: `tools/harness.rs:1120` (the runner executing a python leg). The TLS fixture peer is gone |
| `verification/*.py` | **2 files / 114 lines** — `history_migration.py` 43 (kept: its comparator is owner-supplied and absent), `live_web.py` 71 (kept: not a wired group, and its twin exits 2 asking for `--live`, which the runner does not implement) |
| tracked python overall | **3 files / 229 lines**, of which `tools/package.py` is 115 |
| python in `tools/` | **1 file / 115 lines** (`package.py`) — the other seven the report names are already native |

## Two defects found while measuring, both real

1. **The runner could not fail on a name it would not run** (`tools/harness.rs:1829` filtered `GROUPS`, and a
   name matching nothing printed a summary and exited 0). Found because a delegated census named
   `service_identity` as a red group and that name exists nowhere in the tree. Closed by a guard that exits 2
   naming the declared categories; witness in `/tmp/guardwit.log`, four new unit cases, three mutations each
   naming one red, gate re-run with `harness-tests cases: 32`.
2. **The native TLS fixture was non-conformant and only OpenSSL noticed.** It passed its own JVM subject
   (61 checks, byte-identical report) and failed every `openssl s_client` arm until the RFC 5746 SCSV
   indication (`0x00ff` in the cipher-suite list) was read and the ServerHello echo was emitted with its
   required single `0x00` byte. Details and the mutation battery are in `project-progress.md` and
   `closure_ledger.json`.

## What is actually left, ranked

1. **Re-make the Gap 2 and Gap 3 run logs on these bytes** — `docs/RRSI-LOOP-RUNLOG.md` holds both logs with
   values printed, but at `b8b0e71`, and the cost arm has moved since. The printing half of that is now done:
   the `rrsi_loop` group prints the artifact values itself (27 value lines on the current lane, each labelled
   with the request or run it came from) and its own last check goes red when a print degrades to absent, so a
   room run of the delivered group **is** the run log — only the room run itself is still owed.
2. **Gap 2 #11 and Gap 3 #2/#3** — the k-candidate pool and composition are the largest genuinely un-witnessed
   piece of the recursion story.
3. **Gap 4 #2/#3/#4** — the AArch64 instruction IR exists and is wired into the control-flow report, but no
   group asserts its report text (`0` hits in `verification/`), structured pseudocode does not exist, and the
   JS/.NET evidence has no verification group; all three are uncertified rather than missing.
4. **Gap 5 residuals** — `live_web.py` needs the `--live` route built before it can be retired;
   `history_migration.py` needs an owner-supplied comparator; `tools/package.py` stays until a signed
   candidate is published with it, and `PUBLICATION-MANIFEST.json`'s stale pins are regenerated only by a real
   publish, which is owner-gated on the signed companion APK.
