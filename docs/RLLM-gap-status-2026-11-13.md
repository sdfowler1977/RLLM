# RLLM gap status, measured on the committed bytes (2026-11-13)

**Against:** `docs/RLLM-recursion-gap-report.md` (2026-11-11). That report states its own evidence is
**static** — "Every finding comes from reading the current RLLM source tree, not from running it. The tree
has no git metadata." Several of its ranked gaps have moved since, and two of its "missing entirely" claims
are now contradicted by measurement. This document records what I ran today, on which instrument, and what I
did **not** measure.

**Instruments used today (nothing else counts here):** the sanctioned tool gate
(`bash tools/build-tools.sh output/tools` → nine tool suites, each built twice and executed, with an executed-case
count interlock against the `#[test]` count in that source), the verification runner
(`output/harness <group>`, which does **not** rewrite `verification/RESULTS.json`), `tools/build-verification.sh`
(twin builds + the 44 helper cases), and two kinds of git-archive witness room: a **byte-copy room** (`git archive <commit>`, no tracked file from the working tree overlaid, so every compiled file *is* that commit's blob; the untracked `tooling/` toolchain directory is supplied by symlink because it belongs to no commit (`git ls-tree e500c6d --name-only | grep -c '^tooling$'` = 0 entries)) and an **overlay room** (the same extraction with named working-tree files copied over it, used to witness lane bytes that predate a commit — the ledger rows carry the per-path blob join naming exactly which files differ and why). A claim is attributed to the room kind that actually produced it. Rows marked
**NOT MEASURED** are claims I am leaving as claims; they are not green. **CHECK TARGET WITNESSED** means a
delivered native group asserts the behaviour and went green on the named bytes — it does not close the
behaviour as a product claim. Where the asserting check is not in the committed group, the row says so and
gives the arity join instead (see the Gap 1 row: `git show HEAD:verification/self_rebuild.rs` declares
`const NAMES: [&str; 9]` at `:29` and the byte-copy room's sweep runs `self_rebuild PASS (9 checks)`, while
the working tree declares `[&str; 12]` at `:42`). A witness room's gate is reported per verb: no room in
these documents is claimed green as a whole tree — the byte-copy room of `065efd3` sweeps to `sweep_rc=1`
with `history_migration` the sole failure. Rows 11 and 12 are measured on commit `065efd3`, which is later than the pin
named below.

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
| 11 | parallel candidates; incumbent kept when none admissible | **CHECK TARGET WITNESSED — the delivered group asserts it and the group is green on these bytes; this is not a closed product-behaviour claim, and the limits are in the ledger row's `not_claimed`** | `CANDIDATE_MAX 3` (`:101`), `WORKTREE_MAX 3` (`:102`), `pool_status` (`:1561`). The `rrsi_loop` group now drives the pool instead of only denying it: four of the five new checks — the fifth, the per-candidate worktree check, is recorded on row 12 — one row per candidate stamped with that run's own digest, two malformed-count refusals carrying the product's own sentences and no row/no task run/no run digest before or after the reply, and a fully refused pool keeping the incumbent byte-equal to its instructions — plus the pre-existing negative row. Room run on the committed lane bytes (`git cat-file -p 065efd31121593ab51bd89a151dd5f7df4ae72c2:verification/rrsi_loop.rs | sha256sum` = `c6994ce5b53eb4abdd638037adacfc08af91cd407f6096c3fd290a180302bc50` = that room's `room_lane_sha`): `rrsi_loop PASS (exit 0, 2s, 23 checks)`, report count 23 failed [] (`/tmp/g30room.sh`, transcript `/tmp/g30room.out`); the values are quoted in `docs/RRSI-LOOP-RUNLOG.md` §Gap 2 #11 and #12. Battery arms `v6`/`v7` show the refusal and row-binding conjuncts can go red. **Falsifiability provenance:** the seven batteries ran on uncommitted lane bytes (the seven logs end in six distinct restore digests; the last battery restored to `daa9f5a3…`); the committed `c6994ce5…`. No battery ran on a committed lane state: the seven logs' six distinct restore digests match neither the parent lane blob (`a9c3e82df860…`) nor the committed one (`c6994ce5b53e…`). The difference between the last battery base and the committed bytes is seat 5's reading of the battery's own base copy (the SCOPE string and one comment line, six changed lines), not a diff reproducible from a git object. **Counting convention:** this row counts the four pool checks that target #11; the fifth new check (the per-candidate worktree check) is counted on row 12; the group total is 23 including the pre-existing negative pool row. |
| 12 | git worktree candidates, incumbent is a commit | **CHECK TARGET WITNESSED — both clauses asserted by the group, on the driven three-candidate pool; simultaneous slot occupancy and candidate-id identity are not claimed** | `require_recorded_worktree` on the `refine` path (`core/harness.rs:480`), `EVOLVE_BRANCH "rllm-evolve"` (`:103`), commit-in-slot (`:1997`), `commit_candidate(root,slot,body,base)` (`core/rrsi.rs:1969`). The group asserts the fast-forward half (`the accepted candidate's commit is the rllm-evolve ref and the worktree HEAD`) and now asserts the per-candidate half with a fifth check: the three slot worktrees that round leaves behind each hold one candidate — slot `.git` file present, `.git/worktrees/cN/HEAD` equal to that candidate's row commit, three pairwise-distinct commits, and that candidate's body in the slot's `rllm-harness.txt`. Arms `v1`-`v4` red name that check. **Claimed scope:** the durable post-round state, not simultaneous occupancy during `evaluate_pool`; `CANDIDATE_MAX = 3` means a pool declaring more than three candidates is refused by that sentence, so `parse_pool` refuses a pool that declares more than three candidates; whether another route could admit a fourth candidate is not surveyed here. The per-candidate body equality is asserted for the three slots of the driven pool; the separate kept-incumbent check narrows its own claim to the c0 copy being one of three named values (absent, the room's harness text, or a body this run submitted), because which slot last wrote a copy is product scheduling state. |
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
**Closed the same day at `095dc334`:** `docs/RRSI-LOOP-RUNLOG.md` now carries a Gap 2 section made **by the
delivered group itself** in a `git archive` room of that commit — budgets 4/3/2 plus the resumed request's
`Edits remaining this round: 1. The annealed budget is 3.`, per-row `noise_floor 0`/`2`, `pruned_components
["skills"]` beside the flat run's asserted absence, the incumbent `868dce24c1df5f5640c4474e81e19f8b01cb9f30`
printed as ledger row, `refs/heads/rllm-evolve` and worktree HEAD against `refs/heads/main` at the planted seed
`65103240f6e7fc83f18720f836f0ba5a925e5bae`, and the held-out task/verifier digests — so the stale-cost-row
objection no longer applies to these bytes, and no python driver was involved.

## Gap 3 — Harness² two-level recursion

| # | item | status | measured evidence |
|---|---|---|---|
| 1 | task-level contrastive PPC | NOT MEASURED | the propose→probe→compose route exists in `core/harness.rs`; I did not witness a contrast-composed successor |
| 4 | **a later task reads the library the earlier task wrote** | PRESENT — asserted by the delivered group, and the run log is now on these bytes (`095dc334`) | witnessed at product level in `docs/RRSI-LOOP-RUNLOG.md` §Gap 3 at `b8b0e71`: two separate `output/headless/RLLM` processes, the second one's propose wire (9,799 bytes) carrying task t's `key 27c0edc8…`, `revision 665268c8…` and `harness_digest 65da7eef…` under `HISTORICAL EXPERIENCE`, and a third run under a different key shown the same entry as `indexed key is from another task`. Measured gap that the read-back lane closed: no delivered group asserted it — `grep -rl "experience.jsonl|library_index|lesson_context" verification/` hit only sealing and tamper arms, and `rrsi_loop` now carries `a later refine task reads the earlier task's sealed experience entry from the propose prompt` and `a refine task whose library holds no sealed row shows no historical experience section`. On `095dc334` the room run prints the entry it is joining on — empty library first (`key absent revision absent harness_digest absent run_digest absent`), then `key b19a20739a344697ec81bae2cb21d8c86d474eef70dfff0fcf971dc0cfc33a6a revision 4fb634b436cc678dbcb917a3e453d73c817096a666f4a7f2acd71a6bd087c6fa harness_digest 79a298286981294143558b1d13f30dbb8b1d64e9b6c643399bdc0bc7c6c89645 run_digest c8a52ebe2857fab0f3ff6a10da4c8fb63f458f963f1ccc7117d642de457d0926` |
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
| python spawned by native code | **1** site: `tools/harness.rs:1132` on HEAD `1225caf` (`:1130` on `e500c6d`; `dd5cff7` moved this site by two lines through its only hunk above it, base `:7`-`:8` growing 2 → 4 lines — `git show dd5cff7 -U0 -- tools/harness.rs` prints hunks at base `:7`, `:1530`, `:1725`-`:1734`, `:1740`, `:1744`, `:1750` and `:2663`, so the other six sit below this site; numstat `328 13`, `wc -l tools/harness.rs` 2,664 → 2,979) — `.arg(root.join(format!("verification/{name}.py")))` — the runner executing a python leg (`leg_source` at `:1139`-`:1146` reads the native twin first and falls back to `.py` only when no `.rs` exists; `group_command` at `:1150`-`:1159` runs the built twin when present; (`6102133^` had it at `:1122`: `git show 6102133^:tools/harness.rs | grep -n 'verification/{name}.py'`, and `6102133` grew that file by 25 lines (2,639 → 2,664); 8 of those insertions sit above this site, so the spawn site moved → from `:1122` to `:1130`, a shift of +8 (`git show 6102133^:tools/harness.rs | wc -l` = 2,639, `git show 6102133:tools/harness.rs | wc -l` = 2,664; `wc -l tools/harness.rs` = 2,664 on `e500c6d`))). The TLS fixture peer is gone |
| `verification/*.py` | **2 files / 114 lines** (`ls verification/*.py | wc -l` = 2, `cat verification/*.py | wc -l` = 114) — `history_migration.py` 43 (kept: its comparator is owner-supplied and absent). Measured today, its **native** twin is reached and executed but cannot complete: `tools/harness.rs:1086` `if name == "history_migration" || (name == "prompt_reference" && controller.is_file())` passes `--previous-controller` to `history_migration` unconditionally, defaulting to `root/output/previous-controller` (`:1085`); that path is never produced on the measured route: the only *writes* of `output/previous-controller` in tracked source are test bodies at `tools/harness.rs:2488`/`:2512` inside `#[cfg(test)]` (from `:1986`), the only other tracked references are the input declaration at `:56` and the default-path construction at `:1085`, and no build script in the measured set mentions it (`grep -rn previous-controller tools/*.sh headless.sh package.sh desktop.sh` → 0 hits). Root cause, read out of the code rather than inferred from the red: `tools/harness.rs:56` declares the input `("--previous-controller", "output/previous-controller", "RLLM_PREVIOUS_CONTROLLER")`; `leg_source` reads `verification/<group>.rs` when that file exists and falls back to `.py` only otherwise, so for this group the input detector reads the **native** twin; the native twin states its requirement in its own parse error (`verification/history_migration.rs:73`, `panic!("the following arguments are required: --previous-controller")`) and the committed detector looked only for a script's `add_argument('<flag>'` declaration — at `065efd3` that detector is `tools/harness.rs:110`, which builds that search string for each declared flag and skips the flag at `:111`-`:112` when the leg source does not contain it — so a native twin's requirement was invisible to it; the harness then hands the twin its default path (`tools/harness.rs:1085` in the working tree, `:1077` with the push at `:1079` in `065efd3`), no tracked build script in the swept list and no tracked non-test source produces that path — the only writes of `output/previous-controller` in tracked source are test bodies at `tools/harness.rs:2488` and `:2512`, inside `#[cfg(test)]` which begins at `:1986`, and `grep -rn previous-controller tools/*.sh headless.sh package.sh desktop.sh` = 0 hits — so the twin spawns that path as a controller binary, panics on the missing file (`could not start /tmp/g39room/output/previous-controller: No such file or directory (os error 2)` in `/tmp/g39room/output/checks/history_migration.log`), and the group ends `FAIL (exit 101, 0s, absent checks)`. The delivered record disagrees with that, and the disagreement is dated rather than argued: the record's entry `"name": "history_migration", "missing": ["--previous-controller reference at output/previous-controller"], "recorded_as": 0` was published at `0eb0cb1` (2026-11-12 19:38Z), and the native twin landed later at `54c1f1c` (2026-11-13 10:22Z) — `git merge-base --is-ancestor 0eb0cb1 54c1f1c` succeeds — so that entry describes the python-twin era, when the detector could read `verification/history_migration.py:3`'s `add_argument('--previous-controller',required=True)`. A fresh room of `065efd3` (the bytes before `6102133`) therefore ended `history_migration FAIL (exit 101, 0s, absent checks)` — the panic is in `/tmp/g39room/output/checks/history_migration.log` —, while the delivered record carried the older unavailable entry with `"history_migration": 0` checks. On `e500c6d` that same group is recorded UNAVAILABLE with rc=0 and neither leg is executed, as the run-log row below witnesses. Owed options, either of which is honest — supply a real previous-controller artifact, or report the group
*unavailable* — and the second is now built in the working tree and witnessed (`/tmp/hmfix.out`:
`harness_build_rc=0`, `hm_rc=0`, `history_migration UNAVAILABLE (not executed, missing: path
--previous-controller reference at output/previous-controller)`, with `prompt_reference` still `PASS
(52 checks)` and `binary_flow` unchanged in its verdict); that `tools/harness.rs` fix is **committed** at
`6102133`, so the input classifier runs before either leg is chosen and the default sweep on the committed
bytes records this group UNAVAILABLE with its missing input named instead of FAIL (witnessed in the block
below), and the group's migration behaviour is still not executed, `live_web.py` 71 (kept: not a wired group — an `OPTIONAL` row at `:38`, refused as a declared optional group before any leg runs, as the test `live_web_is_refused_with_the_optional_reason` (`#[test]` at `:2707`, body `:2708`-`:2723`) shows, and absent from `const GROUPS`, so no sweep leg runs for it; a requested-group call for `live_web` does reach the classifier and is refused there as a declared optional group (`classify_requested_group`, the `RequestedGroup::DeclaredNotExecutable` arm at `:1895`-`:1897`) before any leg is chosen; its twin is still built by the sanctioned verification build (`ls /tmp/g44room/output/verification` contains `live_web`) and the `.py` stays as a fallback leg for a group no runner group names. Its twin exits 2 asking for `--live`; across the tracked Rust sources `--live` occurs in exactly two files — `git grep -c -- --live -- '*.rs'` = `tools/harness.rs:1` (the `OPTIONAL` reason string at `:38`) and `verification/live_web.rs:2` (the twin's own argument parsing) — so no tracked Rust source other than the twin's own parser mentions that flag, and the runner never builds it into an argument for a group it will not run) |
| tracked python overall | **3 files / 229 lines**, of which `tools/package.py` is 115 |
| python in `tools/` | **1 file / 115 lines** (`package.py`) — the other seven the report names are already native |

**Gap 5 disposition, re-measured on `e500c6d` (2026-11-13T23:45Z).**

| what | measurement |
|---|---|
| python under version control | `git ls-files '*.py'` = **3 files / 229 lines**: `tools/package.py` 115, `verification/history_migration.py` 43, `verification/live_web.py` 71 (`/tmp/pycensus3.txt`). The same read at the commit rather than the index: `git ls-tree -r e500c6d --name-only | grep -c '\.py$'` = 3, and `git show e500c6d:<path> | wc -l` = 115 / 43 / 71 for those three paths |
| the verification residue | `verification/*.py` = **2 files / 114 lines**; `verification/*.rs` = **72** twins. `const GROUPS` (`tools/harness.rs:19`-`:33` on HEAD, `:17`-`:31` on `e500c6d`) has **71** entries, **1** of which has a `verification/<name>.py` (`history_migration`); `live_web.py` is not a member of `const GROUPS` — it is an `OPTIONAL` row at `:38` refused with the optional reason, pinned by the test `live_web_is_refused_with_the_optional_reason` (`#[test]` at `:2707`, body through `:2723`); its twin is still built by the verification build (`/tmp/g44room/output/verification/live_web` exists) |
| the ceiling | `tools/package.rs:2200`-`:2201`: `VERIFICATION_PY_FILE_CEILING = 2`, `VERIFICATION_PY_LINE_CEILING = 114`, enforced at `:2443` and `:2449` — a larger count refuses, a smaller one passes, so retiring `history_migration.py` alone moves the pair to 1 / 71. What is enforced is the upper bound; that the pair only ever moves down is the comment's claim at `:2193`-`:2199`, and the two constants are ordinary editable source values, not a self-lowering mechanism. The comment at `:2193`-`:2199` states its own per-group wiring counts; those are the comment's claim, and the wiring census above is measured separately |
| are the python legs live? | Both still run as fallbacks and both name the requirement their twin names: `python3 verification/live_web.py` → `live_web.py: error: the following arguments are required: --live` rc=2 against `output/verification/live_web` → `error: the following arguments are required: --live` rc=2; `python3 verification/history_migration.py --previous-controller output/previous-controller` → `FileNotFoundError ... 'output/previous-controller'` rc=1 against the twin's `could not start output/previous-controller: No such file or directory (os error 2)` rc=101 (`/tmp/pyfallback2.txt`). They are redundant, not load-bearing: the runner picks the built twin whenever it exists (`ls /tmp/g44room/output/verification` contains both `history_migration` and `live_web`) |
| what `6102133` changed for them | `history_migration`'s requirement is now visible to the classifier, so in the default sweep the group is recorded UNAVAILABLE **before** either the `.py` or the twin is executed — the python leg is further from the delivered verdict than it was |
| run log on the committed bytes | `/tmp/g44room.sh` (transcript `/tmp/g44room.out`) extracts `e500c6d` with `git -C /home/shawn/RLLM archive e500c6d | tar -x -C /tmp/g44room`, adding no tracked file from the working tree on top — the single addition is a symlink `tooling` — `/home/shawn/RLLM/tooling`, and `tooling/` is in no commit at all (`git ls-tree e500c6d --name-only | grep -c '^tooling$'` = 0 entries; `git ls-files tooling` = 0 files reads the index rather than a commit, so the tree read is what settles it), holding the toolchain the sanctioned runner reads (`ls tooling` → `buildtools`, `components`, `downloads`, `env.sh`, `jdk`, `ndk`, `platform`, `rust`) (`room_has_git=no`), so every tracked file the row's numbers come from is that commit's blob: the committed record's blob id is `1450f95863f9cc05b3c61e95ce7941d7f3154855` (`git -C /home/shawn/RLLM rev-parse e500c6d:verification/RESULTS.json`) and that file's sha256 in the room before any room run is `e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`. On those bytes `headless.sh` rc=0 with 0 warning lines; `tools/build-tools.sh output/tools` rc=0 with 10 `cases:` lines, 0 `^error` lines, and `tool tests: output/tools/harness-tests cases: 33` at `output-tools.log:19` (the `6102133` case is inside the committed case count); `tools/build-verification.sh .` rc=0 with 73 binaries in `output/verification` — the 72 `twin:` lines of `output-verify.log` plus `output/verification/twin-helper-tests` — and 0 warnings; `tools/build-harness.sh` rc=0. Then `output/harness history_migration` → rc=0, its log `/tmp/g44room/group-hm.log:1` reading `history_migration UNAVAILABLE (not executed, missing: path --previous-controller reference at output/previous-controller)` and `:2` reading `summary: 0 pass, 0 fail, 1 unavailable of 71 groups` — on the committed bytes that group is recorded unavailable with its missing input named, neither the `.py` nor the twin is executed, and the run exits 0 rather than red. The same room runs `output/harness self_rebuild` → rc=0 with `/tmp/g44room/group-self-rebuild.log:1` reading `self_rebuild PASS (exit 0, 3s, 12 checks)` and `:2` reading `summary: 1 pass, 0 fail, 0 unavailable of 71 groups`. |
| whole-tree sweep on those bytes | The same room runs `output/harness sweep` → rc=0: `/tmp/g44room/output-sweep.log:73` `delivered record written: 71 groups, 4157 checks`, `:74` `summary: 64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4157 checks`. No group fails on the committed bytes. The seven the room records unavailable (`output-sweep.log:11,14,50,51,52,65,69`) are `binary_flow` (three missing controller reference builds), `history_migration` (`output/previous-controller`), `field_flow`, `flags_flow`, `product_flow` (each a missing `output/published-controller` reference build), `apk_alignment` and `turn_background` (the companion APK `RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk` named by `PUBLICATION-MANIFEST.json`, not on this machine). The room writes only its own record: its `verification/RESULTS.json` sha256 moves `e2f35542e44b5509…` → `08949fd8dee28d97…` while the repository's copy is unchanged by that room run at `e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`. The published record disagrees with that fresh sweep: 4,248 checks, 67 groups passed, 4 unavailable (`apk_alignment`, `binary_flow`, `history_migration`, `turn_background`) — `field_flow`, `flags_flow` and `product_flow` are missing from its unavailable list, and its check total is 91 higher than what the committed bytes produce in the room. That disagreement has a measured cause, now witnessed by an arm run in the same room rather than read off the current tree: `output/published-controller` is an owner-supplied reference that lives outside version control (`git ls-files output` = 0 files tracked, while the live tree holds it at `-rwxrwxr-x 11,809,824 bytes`, mtime 2026-11-11 10:09), and no build or publish verb produces it (`grep controller headless.sh package.sh desktop.sh tools/*.sh` = 0 hits) — the publisher only consumes it and refuses without it (`tools/package.rs:2799`-`:2800`, `refuse("output/published-controller absent")`), the only tracked write being a test body at `:3638`. The groups spawn it as the prior published controller for a comparison leg (`verification/field_flow.rs:221`-`:232`, `verification/binary_flow.rs:222` and its `--published-controller` flag at `:243`-`:247`). So the published record is self-consistent: walking `checks` (a dict of 71 group rows) the `recorded_as: 0` rows are exactly those four, while `field_flow`, `flags_flow` and `product_flow` carry 37, 28 and 37 passed checks with `missing: null` — they ran at publish time, which a byte-copy room by construction cannot reproduce. The arm: that same file was copied into the room and `output/harness field_flow` was run there after the sweep quoted above — the copy is the arm's addition, not part of the recorded byte-copy room — giving `field_flow PASS (exit 0, 0s, 37 checks)` and `summary: 1 pass, 0 fail, 0 unavailable of 71 groups` with rc=0 (`/tmp/g44-field-flow.log:1`-`:2`) — the room row moves from UNAVAILABLE to green on that one added input, and its 37 checks equal the record's own `connected.counts["field_flow"] = 37`. The record witnesses the publish-time state directly as well: `connected.counts` gives `field_flow` 37, `flags_flow` 28 and `product_flow` 37 against `total_executed: 4248`, `groups_passed: 67`, `groups: 71`, and its `zero_count_disclosure` names exactly `binary_flow, history_migration, apk_alignment, turn_background` as the zero rows. One thing the digest does not witness, stated so it is not read as more than it is: the file's current sha256 is `97a42c14bd73cf880404c7afcc241a9942062932f983b6cdbf9b2eb4e1099b7a`, which is not the record's `published_comparator.controller_sha256` `d3cc95bdc004549eea0a83a637e448713f6d45252fff4cfa72c518d66ff0cb99`, so these bytes are not claimed to be the bytes the published record ran against. |
| `tools/package.py` | a declared compatibility role: `TOOL_PYTHON` at `tools/package.rs:2186`-`:2191` — "publisher until a signed candidate is published with the native package binary" |
| what blocks retiring the last two | `PUBLICATION-MANIFEST.json` pins **75** `.py` paths in its `files` section — **72** absent from the worktree and untracked, **3** still tracked (`tools/package.py`, `verification/history_migration.py`, `verification/live_web.py`); re-walked 2026-11-14 by key-listing both sections, which gives `files` 281 keys / 75 `.py` and `build_inputs` 162 keys / 1 `.py` (`tools/alignment.py`, absent) — of the three present, `verification/history_migration.py` (`bccf36d21eb3…`) and `verification/live_web.py` (`6152f050d3ec…`) match their pins while `tools/package.py` does not (pin `e4ea98ca9dff…`, current `fc7670b7b19a…`). Its connected pins are stale in the same way: manifest 65 pinned groups summing 4228 (`groups: 65`, `groups_passed: 64`, `unavailable: [binary_flow]`) against the record's 71 groups / 4248 / 67 with four unavailable rows; 8 group pins differ (`apk_alignment` 8→0, `binary_flow` 94→0, `commands` 79→86, `history_migration` 7→0, `pe_declarations` 18→43, `product_flow` 30→37, `prompt_reference` 56→52, `services` 69→72) and 6 groups exist only in the record (`desktop_runtime`, `permission_modes`, `rrsi_loop`, `self_rebuild`, `state_sealing`, `turn_background`); `files` has 281 entries and `build_inputs` 162 (one, `tools/alignment.py`, untracked), with 145 tracked paths pinned nowhere (`/tmp/manifest-staleness2.txt`, which contains a superseded first block that died on a `TypeError` and the working join below it). The publisher's own census of that manifest, run in the room of `e500c6d`: `files: pinned 281 match 156 drift 53 absent 72` (`/tmp/g44-publish-A.txt:2`), and `absent 73` with `verification/live_web.py` moved out of the room (`/tmp/g44-publish-B.txt:2`) — the instrument counts a missing pinned file, it does not refuse on it |
| therefore | retiring either verification `.py` today would leave the publication manifest pinning a file that no longer exists — that is the state the manifest is already in for 72 of its 75 pinned `.py` paths, measured by re-listing those pins against `git ls-files` —, and the manifest carries the signed-candidate fields as measured — `state` is `"in_progress; signed candidate"`, `whole_project_complete` is `false`, pinning the companion APK at `signed_apk.path` = `RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk` (`signed_apk.sha256` = `86b7d74156a11dc6ee6d70866bb480e30b75bac57c3400878c86b9f205921c3d`, 6,932,498 bytes) and carrying the top-level `signing_certificate_sha256` = `ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5`. The manifest names the signed companion APK and its signing certificate; that this lane performed no republish is a scope statement about this document and who republishes is program governance, neither of them a measurement — so on the bytes this document measured the retirement has not been performed, and no schedule for it is claimed here |

Claimed: the residue is 2 files / 114 lines under ceilings that refuse any larger count (the one-way direction is the comment's claim, not an enforced property); both legs are reached only as the fallback — `group_command` (`:1150`-`:1159`) runs the built twin when that binary exists and takes the `.py` otherwise, and the room build of the committed bytes produced all 72 native twins (73 binaries in `output/verification`, counting `twin-helper-tests`; re-counted 2026-11-14 in a clean `git archive` room of HEAD `1225caf` — `grep -c '^twin: '` = 72 plus one `twin helper tests: /tmp/g66room/output/verification/twin-helper-tests cases: 44` line in `/tmp/g66-committed.log`); the residue is not implicated in the cited `history_migration` unavailability (the record and the room both attribute it to the missing input), and no red verdict in the measured artifacts is attributed to it. Not claimed:
that the legs are unreachable (they run when a twin binary is absent), that retiring them would change a verdict,
or that the manifest can be rewritten without the owner's publication step.

## Two defects found while measuring, both real

1. **The runner could not fail on a name it would not run** (`tools/harness.rs:1829` as numbered in the tree where it was found; on `e500c6d` that filter is
   `classify_requested_group` at `tools/harness.rs:1817`-`:1828`, its `GROUPS` test at `:1818`-`:1820`, and a
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

0. **Closed today.** The Gap 2 and Gap 3 run logs now exist on the committed bytes and are made by the
   delivered group itself, not by a driver: a `git archive` room of `095dc334` built its own product
   (`headless_rc=0`, 0 warning lines), its own twins (`verify_rc=0`, 72 twins, `twin helper tests … cases: 44`)
   and its own runner (`bash tools/build-harness.sh` → `harness_build_rc=0` — the only verb that produces
   `output/harness`), then `output/harness rrsi_loop` → `PASS (exit 0, 1s, 18 checks)` with 27 printed value
   lines, copied into `docs/RRSI-LOOP-RUNLOG.md` §§ "Gap 2 again…" and "Gap 3 again…".
1. **Gap 2 #11 and #12 are witnessed as check targets on these bytes (on the driven three-candidate pool, with the limits recorded in the ledger row); they are not closed as product-behaviour claims, and the measured token census found zero occurrences under `verification/` of the seven lower-case tokens this row names (`reflection`, `invariant`, `domain_tag`, `deliverable`, `edit_keys`, `policy_keys`, `split_keys`); it does not rule out an equivalent check written under other names.** (Measured: `grep -rl reflection verification | wc -l` = 0 files, `invariant` = 0, `domain_tag` = 0,
`deliverable` = 0, and the same census for the sealed-row key names `edit_keys` = 0, `policy_keys` = 0,
`split_keys` = 0. Re-measured on this working tree for the whole word (in-process census over `git ls-files`, artifact `/tmp/deliverable-census.txt`) it gives 31 occurrences in 6 tracked files — `closure_ledger.json` 15, `docs/RLLM-gap-status-2026-11-13.md` 10, `docs/RLLM-recursion-gap-report.md` 2, `project-progress.md` 2, `docs/AUDIT.md` 1, `docs/RRSI-LOOP-RUNLOG.md` 1 — and 0 under `verification/`. Four of those six files are the claim files of this row, so the count includes this row's own prose; an earlier count of 17 in six files was quoted from a state that no on-disk witness reproduces and is not used here.) The k-candidate pool is now a check target: `rrsi_loop` carries five pool checks — one ledger row per candidate stamped with that run's own digest, two malformed-count refusals, the fully-refused pool keeping the incumbent byte-equal to its instructions, and the three slot worktrees that round leaves behind holding one candidate each — witnessed on the committed lane bytes `c6994ce5b53e` in a git-archive room: `rrsi_loop PASS (exit 0, 2s, 23 checks)`, report count 23 with `failed` empty (`docs/RRSI-LOOP-RUNLOG.md` §Gap 2 #11 and #12). What that did *not* close is the composition half: the sealed-row vocabulary carries no deliverable / reflection / invariant / domain field — read off the three enumerated key arrays (`EDIT_KEYS` `core/rrsi.rs:58-69`, `POLICY_KEYS` `:70`, `SPLIT_KEYS` `:73-91`) plus the 0-file census above, not off a whole-product field scan. On those two measurements the honest
statement is that no delivered check targets that layer and the sealed-row vocabulary carries no field for it;
reading that as "absent from the product" is a token-and-array census, not a proof about naming, and it is
recorded as owed work rather than as a closed product fact.

2. **Gap 1 whole chain on a native driver — in flight, and now honestly bounded.** Relative to the committed version of the group (`git rev-parse 065efd3:verification/self_rebuild.rs` = blob `46a965feecb7beca5b17864479a70933bb182ef1`), the working-tree native group has a tenth check, `review_confirmed handoff re-execs the room successor and the same pipe answers the room verb`, green on its own build (`10 self-rebuild checks passed`, transcript `/tmp/srrun1.log`), asserting a same-pipe answer over an empty successor set; it does not itself commit the edited source or continue a carried job, and the python-driver witness (`docs/SELF-BUILD-RUNLOG.md`, candidate `1cbc272`) remains the only whole-chain witness cited here. An audit seat read those bytes and refused the broader claim (`/tmp/mm-audit-selfbuild.out`, Verdict: ACCEPT WITH NOTES, notes N1/N3/N8 — transcript quoted in the audit packet): the leg neither commits the edited source nor continues the carried job after exec, and its `successor_rows`/`successor_tip`/`successors` assertions check field shapes on an empty successor set. A product `self_commit` verb and an eleventh check for it exist in the working tree and are witnessed in an overlay room: `/tmp/g31room.sh` extracts `git archive HEAD` (HEAD was `065efd3`) into `/tmp/g31room` and copies the three lane files `core/lib.rs`, `core/self_build.rs`, `verification/self_rebuild.rs` over it, with `tooling` symlinked, and its per-file digests are in `/tmp/g31room-out/overlay-shas.txt`. Its transcript `/tmp/g31room.out` ends `G31-DONE` with `twin_build_rc=0 twin_warnings=0` and `lane_rc=0`; the passed line is in the lane log, not the transcript — `/tmp/g31room-out/self_rebuild.log:15` = `11 self-rebuild checks passed`, with `"count": 11, "failed": [], "passed": 11` in the same file. Its battery `/tmp/srbatt-run.log` has 10 arms: `w0` green (control, `names-check11=yes`) and `w1`—`w9` each `rc=1` naming the check `a review permit commits the journalled revision and the commit tree carries the edited source bytes`. The file now carries a twelfth check, `a review permit refuses a same-revision decoy commit and commits the tracked source tree above it` (`verification/self_rebuild.rs` in the working tree: `const NAMES: [&str; 12]` at `:42`, the twelfth name at `:54`), whose fixture stages `notes/decoy.txt` under `git add -f` and `output/decoy.bin`, then deletes both from the worktree, so the tracked path set is larger than the head tree set while `git diff --quiet HEAD` still reports clean; the product commit path in `core/self_build.rs:1232` compares `commit_tree_paths(root, git_dir, &head, skip)?` against the index path list and refuses such a commit (`matching_commit` returns `Ok(None)`), and the check requires the permit to produce a commit whose parent is the decoy commit and whose blob bytes equal the edited source. That guard is measured load-bearing in a a `git archive` room of `065efd3` with the three lane files copied over it (an overlay room, not a byte-copy room) (`/tmp/g42room.sh`, transcript `/tmp/g42room.out`, logs `/tmp/g42room-out/`): the baseline group is green with `12 self-rebuild checks passed` and `"count": 12, "failed": [], "passed": 12` (`/tmp/g42room-out/self_rebuild-baseline.log:18`); replacing the guard's `if commit_tree_paths(...)? != paths { return Ok(None); }` with `if false {` (room file sha256 `0c83ad4d8262…`) makes exactly one check red — `GROUP[mut] rc=1`, `fail_lines=['FAIL a review permit refuses a same-revision decoy commit and commits the tracked source tree above it']` (`/tmp/g42room.out:18-20`, `/tmp/g42room-out/self_rebuild-mut.log:17`) — and no other check; an inert control that inserts a comment line above the guard (room file sha256 `8d0b4480f14e…`) keeps `GROUP[inert] rc=0` with `12 self-rebuild checks passed` (`/tmp/g42room.out:27-28`); the guard file is restored afterwards with `RESTORE-SHA b3d08984df61… unchanged=True` (`/tmp/g42room.out:34`), and the room's `self_rebuild.rs` and the repo's are the same bytes (`ceb4748742f7…`, `REPO-EQUALS-ROOM True`). The battery restores the file to its own base bytes (`BASE-SHA 49f6bf130afc…`, `RESTORE-SHA 49f6bf130afc… unchanged=yes`) — that is the base of that battery, not the file as it now stands: the working tree `verification/self_rebuild.rs` is sha256 `ceb4748742f7…` (2,154 lines, `+1054/−11` over `065efd3`) because a twelfth check was added to it after that battery ran; the `065efd3` blob of that path hashes to `2a503b53069d…`, so the restore proves no mutant survived, not that the battery ran on committed bytes, but they are **not committed** — the arity join above is the proof of that — and the continuation leg is not
built, which is measured rather than asserted (`grep -rn carry_on --include='*.rs' core` = 0 hits, and the `self_*` dispatch surface is `core/lib.rs:440-444`: five verbs — `self_fingerprint`, `self_plan`, `self_journal`, `self_handoff`, `self_commit` — none of which continues a round; that census covers the dispatch surface and the `carry_on` token and does not exclude a continuation leg under another name). `core/self_build.rs` has no git-push verb — its only `fn push` is the output-buffer accumulator at `:518`, and `grep -rn '"push"' --include='*.rs' core` returns 3 hits, all `core/page_program.rs:197`, `:228`, `:247`, which are JS array-method dispatch. Until they land, the only whole-chain witness for Gap 1's edit → build → gate → **commit** → re-exec →
carry-on chain is the python-driver witness (`docs/SELF-BUILD-RUNLOG.md` §"The whole chain again, on the bytes this document is committed against — 2026-11-13T12:30Z, commit `1cbc272`"), which is an R-rule deviation, not a delivery.

3. **Gap 4 #2/#3/#4** — the AArch64 instruction IR exists and is wired into the control-flow report, but no
   group asserts its report text (`0` hits in `verification/`), structured pseudocode does not exist, and the
   JS/.NET evidence has no verification group; all three are uncertified rather than missing.
4. **Gap 5 residuals** — three `.py` files are still tracked (`git ls-files '*.py'` prints `tools/package.py` 115, `verification/history_migration.py` 43, `verification/live_web.py` 71); the two `verification/` legs each have a measured stop below, and the third has no measured stop at all.
   A dry-run publish run inside the byte-copy room of `e500c6d`
   (`output/tools/package --dry-run --destination /tmp/g44-pub`) prints `build_inputs: pinned 162 match 117 drift
   44 absent 1` and `files: pinned 281 match 156 drift 53 absent 72`, then `refusing: output/checks/apk.json
   absent` with rc=1 (`/tmp/g44-publish-A.txt:1-3`). Moving `verification/live_web.py` out of that room moves the
   same command's second line to `files: pinned 281 match 155 drift 53 absent 73` and it refuses at the same line
   (`/tmp/g44-publish-B.txt:1-3`, rc=1), so **that publisher counts a missing pinned file in its census instead of
   refusing on it** — the only refusal this instrument measured is an unrelated missing input: the census row for a
   missing pin does not stop it. What does stop the publish route is
   `output/checks/apk.json`, and its producer is `tools/apk.rs:1577` (`let destination = out.join("apk.json")`),
   which writes nothing until the APK passes `verify_signature` (`tools/apk.rs:1167`): that function resolves
   `apksigner` through `env_key("CLIENT_TOOLS")` (`:711`), runs `apksigner verify --verbose --print-certs`, returns
   `APK signature verification failed: <name>` when the status is not success (`:1180`), and then `classify`
   (`:1151`) demands `Number of signers: 1` and a certificate equal to `const CERTIFICATE` at `:650` =
   `ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5`, the same value the manifest carries as
   `signing_certificate_sha256`. The manifest's `signed_apk` is `{location: companion, path:
   RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk, bytes: 6932498, sha256:
   86b7d74156a11dc6ee6d70866bb480e30b75bac57c3400878c86b9f205921c3d}`, that companion arrived on this machine after those arms ran, at
   `APK/RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk`, 6,932,498 bytes with sha256
   `86b7d74156a11dc6ee6d70866bb480e30b75bac57c3400878c86b9f205921c3d` — byte-for-byte the manifest's `signed_apk.bytes` and `signed_apk.sha256` — and the pinned verifier says
   the identity holds: Verifies Verified using v2 scheme (APK Signature Scheme v2): true Verified using v3 scheme (APK Signature Scheme v3): true Number of signers: 1 Signer #1 certificate SHA-256 digest: ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5. Two other measured facts keep the route stopped anyway. (i) The record's
   companion probe (`tools/harness.rs:212`-`:228`) looks at the repository root and at `output/*/<name>`, never at
   `APK/`, so the record's `connected.unavailable` row still reads
   `companion APK RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk named by PUBLICATION-MANIFEST.json
   (not on this machine)`. (ii) The product's own apk instrument refuses before it reaches signature verification
   without a sibling list: given that APK it printed `FileNotFoundError: [Errno 2] No such file or directory:
   '/home/shawn/RLLM/APK/build-inputs.sha256'` at rc=1 and wrote no record (`/tmp/g49-apk-real.txt:1`), because
   `tools/apk.rs:1487`-`:1502` reads `<apk dir>/build-inputs.sha256`, validates each `<digest>  <name>` record
   against the tree and then refuses if the inventory changed — that file is what `package.sh:111` writes for a build the product performs (`printf '%s\n' "$build_source" > "$out/build-inputs.sha256"`, identical in the `46e392e` blob of that script). Forcing the subject with the remedy the twin itself names
   (`RLLM_CHECK_APK=<abs path> output/harness apk_alignment`) does not make the leg green either: it runs and
   fails — `apk_alignment FAIL (exit 101, 0s, unparsed checks) |   apk_alignment:  | thread 'main' (1814688) panicked at RLLM/verification/apk_alignment.rs:333:9: | every instrument that reads an APK subject resolves it from the publication record instead of a default path it never verifies | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace | summary: 0 pass, 1 fail, 0 unavailable of 71 groups` (`/tmp/g49-apkalign.log:1-3`) — on the check at `verification/apk_alignment.rs:37`-`:46`, whose
   reader census `verification_readers` (`:196`-`:212`) enumerates `verification/` entries whose extension is
   `py`; on this tree there are 2 such files and `grep -c RLLM_CHECK_APK` counts 0 in each (`verification/history_migration.py:0 verification/live_web.py:0`), so
   `!readers.is_empty()` is false and the check cannot pass whatever the APK does. That is an instrument defect,
   witnessed by that run rather than inferred, and the history of that census is itself in the history: it was
   written at `54c1f1c`, and at `a9e8f11^` the files it matched were `verification/apk_alignment.py` (4
   `RLLM_CHECK_APK` mentions, 4 `PUBLICATION-MANIFEST` mentions) and `verification/turn_background.py` (2
   and 2); `a9e8f11` (2026-11-13) deleted both — `verification/apk_alignment.py verification/turn_background.py` — which is what left `readers` empty on those bytes; the repair and its arms are in the arm set below. Of
   the APKs under the repository the instrument wrote no record and returned rc=1 for every path: that log is 30
   lines (`1` header + `27` probe rows + blank lines), `grep -c 'rc='` counts **27** rows and `grep -c 'rc=1'`
   counts the same **27**, of which 26 rows read `APK signature verification failed` (`grep -c` over the file) and
   one is the companion's missing-sibling-list refusal quoted above (`/tmp/g44-apkscan.txt`).
   In that repository state `ls -l output/checks/apk.json` printed `No such file or directory`; the arms below are what produced that file. This machine's own APK does not satisfy that
   verifier either: with the pinned toolchain exported, the room-built instrument given
   `output/2026-11-13__11-33-43Z/aligned.apk` printed `ValueError: APK signature verification failed:
   local-aligned.apk` rc=1 and wrote no record (`/tmp/g44-apk.txt:1`, reproduced as `ValueError: APK signature
   verification failed: probe2.apk`); sourced through `/bin/sh` instead of bash, the same instrument stops
   earlier and never reaches verification, because that shell mangles the tooling root the script exports —
   (`/tmp/g44-apk-rerun.txt` holds that row.) A second naming of the first arm (`probe2.apk`) was run but no
   artifact of it survives under `/tmp`, so only the `local-aligned.apk` row is claimed here:
   `CLIENT_TOOLS=/tmp/g44room/buildtools/android-15`, a room path that does not exist — giving
   `FileNotFoundError: [Errno 2] No such file or directory:
   '/tmp/g44room/buildtools/android-15/apksigner'`, with the sourcing failure itself reading
   `/bin/sh: 1: /home/shawn/RLLM/tooling/env.sh: Bad substitution`.
   That arm was re-measured 2026-11-14 without a pipe, because an earlier draft recorded `$?` from `head` rather
   than from the shell and its `sh_rc=0` was therefore a phantom: `(sh -c '. /home/shawn/RLLM/tooling/env.sh';
   echo "sh_rc=$?")` prints `sh: 1: /home/shawn/RLLM/tooling/env.sh: Bad substitution` and then `sh_rc=0`. The
   measured property is the pair itself — dash reports the bad substitution and still exits 0, so sourcing the
   instrument under `/bin/sh` leaves the toolchain variables unset while its exit status claims success.
   On those bytes the publish route stopped at the `read_required` at `tools/package.rs:2716` before it ever reached
   the `published-controller` read at `:2799`-`:2800`; the arms below move that stop past `:2716` and past the
   regression read at `:2757`. The rewrite of the manifest pins is one code path: the
   publish plan upserts `files` at `tools/package.rs:2981` (`upsert(&mut manifest, "files", Json::Obj(file_records))?;`) and `build_inputs` at `:2986` (`upsert(&mut manifest, "build_inputs", build_input_json)?;`), hands the rewritten
   manifest to `Plan` as an artifact at `:3056`, and `Plan::commit` (declared at `:2679`) writes that artifact with the
   generic `fs::write` at `:2686` — `rewritten_output` at `:2671`-`:2675` lists `PUBLICATION-MANIFEST.json` among the
   files the publish restamps. Every other mention reads it: that grep hits 4 tools/harness.rs |      20 tools/package.rs |       1 tools/readback.rs, and the only `fs::write` calls
   naming the file by name are the publisher's own test at tools/package.rs:3491, inside `fn build_inputs_census_counts_match_drift_absent()` declared at `:3467`, and that test's fixture at tools/package.rs:3692, inside `fn fixture() -> (TempDir, PathBuf)` declared at `:3619`. A dry run prints
   `would write:` instead (`push_preview` declared at `:2703`, printing at `:2704`); both arms above printed none (`/tmp/g44-publish-A.txt:0 | /tmp/g44-publish-B.txt:0`), and the room's manifest still hashes
   `35884d847b04eae66ca449588b97221a5f8bb0e74be0c91935b957e230156df2`. That equality is measured, not assumed:
   `git cat-file -p e500c6d:PUBLICATION-MANIFEST.json | sha256sum` prints
   `35884d847b04eae66ca449588b97221a5f8bb0e74be0c91935b957e230156df2  -` and `sha256sum PUBLICATION-MANIFEST.json`
   in this tree prints the same digest, so those arms wrote nothing. What is measured there ends at the missing `output/checks/apk.json`.
   The signed-identity chain has since been run on committed bytes, in two trees, and the census defect quoted above
   is repaired on the committed bytes at `69bc535` ("Read the APK-subject reader census from the native legs, with the
   needle that keeps it fail-closed"): `verification_readers` (`verification/apk_alignment.rs:199`-`:215`) keeps
   `verification/` entries whose extension is `rs` (`:204`), and the default-path needle is split at `:36`-`:37`
   (`let owned_form = String::from("RLLM_CHECK_APK'") + ",";`) so the file that states the guard is not itself
   counted as a reader. Its arms, one labelled log each, all in the clone `/tmp/laneC` with the signed candidate and
   its sibling list as the subject (`/tmp/laneC-verify.log`, `/tmp/laneC-armG.log`): (A) no subject — `apk_alignment
   UNAVAILABLE (not executed, missing: path companion APK ... (not on this machine))`; (B) subject supplied —
   `apk_alignment PASS (exit 0, 5s, 11 checks)`; (D) the split needle un-rejoined (the guard's own source carries
   `RLLM_CHECK_APK',` again) — `apk_alignment FAIL (exit 101, 0s, unparsed checks)` with `panicked at
   RLLM/verification/apk_alignment.rs:336:9`; (E) the same addition as a comment only — `apk_alignment PASS (exit 0,
   4s, 11 checks)`; (G) a different real reader, `verification/turn_background.rs`, carries the unsplit default form
   while the twin is rebuilt (`carries the default form (grep -c): 1`, mutant sha `39472389b0369870be5db7f9675d41a9059f5ec012c9853296b0a1e97aab1e6d`) —
   `apk_alignment` red while the reader carries it (`apk_alignment FAIL (exit 101, 0s, unparsed checks)`, `armG_rc=1`), green after the restore (`apk_alignment PASS (exit 0, 4s, 11 checks)`, `restore_rc=0`); after restoring the file (`restored sha` equal to the pre-mutation `fe2cd170a8f9526e1c6a71c119daf7e886d4456220094bc6ccb3ed7f220ac652`)
   the group is green again. `turn_background PASS (exit 0, 0s, 28 checks)` with the same subject. Through every arm
   `verification/RESULTS.json` stayed `e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`. The first
   attempt at arm G is recorded as a void leg, not a pass: it asserted an anchor that the file does not have
   (`AssertionError: 0` at `/tmp/laneC-verify.log:35`) and its PASS proved nothing, so the arm was re-run with the
   mutation inserted above `mod twin;` and with `bash tools/build-verification.sh .` between the mutation and the
   verdict. In a real clone of
   `46e392e` (`/tmp/g50room`) `package.sh` with the retained custody (`CLIENT_VARIANT=lab`, and the room's own
   `version_code=1794270238` as its run header records at `/tmp/g50-package2.log:1`) printed `Signer #1 certificate SHA-256 digest:
   ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5` with `package_rc=0`
   (`/tmp/g50-package2.log:135`, `package_rc=0` at `:144`); that room's own apk instrument then took the APK with the sibling list the
   build wrote beside it and returned `apk_rc=0` (`/tmp/g50-chain.log:225`), writing the room's
   `output/checks/apk.json`, and the dry-run publish advanced past `apk.json` to the next required input —
   `refusing: output/checks/selected-units.json absent` with `pub_rc=1` (`/tmp/g50-chain.log:273`, `:274`). That
   room's instrument report carries `"version": 1794270238` (`/tmp/g50-chain.log:8`, `:230`) while this tree's report
   carries `"version": 1794270239` (`/tmp/g61-apk-main.txt:2`): the two builds used different `CLIENT_VERSION_CODE`
   values, and on those two legs the number did not change the identity verdict: the 1794270238 build returned `apk_rc=0`
   (`/tmp/g50-chain.log:225`) and the 1794270239 build returned rc=0 with `"signature": "v2/v3; one pinned current
   signer"` (`/tmp/g61-apk-main.txt:32`), both against the same pinned certificate digest. A leg that varies only the
   `--version` argument is inconclusive on this tree today: `output/tools/apk --version 1`, `--version 9999999999` and
   `--version 1794270239` all returned rc=1 printing `AssertionError: core/application.rs` on 2026-11-14, because the
   instrument stops at the sibling-list check while `core/application.rs` is dirty in this tree. That
   sibling list is bound to the tree that wrote it: placed beside this tree's copy of the same bytes it made the
   instrument print `ValueError: '/tmp/g50room/AndroidManifest.xml' is not in the subpath of '/home/shawn/RLLM'` at
   rc=1 for both APKs (`/tmp/g58-apk-new.txt`, `/tmp/g58-apk-old.txt`), which is why the chain was then run in this
   tree: `package.sh` produced `/home/shawn/RLLM/output/2026-11-14__01-46-09Z/RLLM.apk` with that same pinned
   certificate (`/tmp/g59-package.log:134`, `:142`, `package_rc=0` at `:143`), and `output/tools/apk --version
   1794270239` returned rc=0 reporting `"version": 1794270239`, `"sha256":
   "c7bfec698e68315bfaf5f618e0453a366b811ab3f39f0966f28b73d76a39e9d2"`, `"signature": "v2/v3; one pinned current
   signer"`, `"zip_crc": "passed"` (`/tmp/g61-apk-main.txt:2`, `:5`, `:32`, `:33`) and wrote this tree's
   `output/checks/apk.json` (17,909 bytes). The dry run now refuses on group state rather than a missing input:
   `refusing: binary_flow=1, history_migration=1, apk_alignment=1, turn_background=1` at rc=1 (`/tmp/g61-pub.txt`),
   because `candidate_gate` (`tools/package.rs:1908`-`:1917`) tolerates exactly one failed row — the `binary_flow`
   row matched by `exact_binary_flow` (`:1900`) with keys exactly `{name, code}`, its gap shape checked by
   `binary_flow_gap` (`:1919`) — while `output/checks/current-regression.json` (3,706 bytes, read at `:2757` and
   parsed by `load_runs` at `:2617`) holds 71 rows whose non-zero entries are those four. That record is dated
   2026-11-12, before the companion arrived and before the census repair: on the repaired bytes with the subject
   supplied, `apk_alignment` and `turn_background` are green (11 and 28 checks), `history_migration` stays
   UNAVAILABLE on `output/previous-controller`, and `binary_flow=1` is the one tolerated gap. So on the current bytes the route stops at the group-state gate, not at `apk.json`.
   That missing producer has since been closed on committed bytes, and the numbers above are scoped to the commit
   they were measured on: at `69bc535`, `grep -c regression tools/harness.rs` counted **0** and
   `grep -l current-regression` named the path in **four** tracked files (`closure_ledger.json`, `docs/AUDIT.md`,
   `tools/package.py`, `tools/package.rs`), the only write being the publisher's test fixture at
   `tools/package.rs:3665`. Commit `dd5cff7` ("Give the sweep a native producer for the publisher's regression
   record, with the refusal that keeps the prior bytes") put `REGRESSION_NOT_PASSED` at `tools/harness.rs:1538`,
   `regression_code` at `:1540`-`:1548`, `regression_document` at `:1550`-`:1570` and `publish_regression` at
   `:1572`-`:1589` into the sweep path (`finish_sweep`, declared at `:1785`, calls `publish_regression` at `:1791`
   and gates `commit_sweep` — declared at `:1684` — on its result at `:1792`), so
   `output/harness sweep` now writes one `{name, code}` object per `const GROUPS` entry. On HEAD the same grep
   names **five** tracked files — the fifth being `tools/harness.rs` itself — and `grep -c regression
   tools/harness.rs` counts **46**. What the runner writes per group is unchanged in kind: one report under
   `output/checks/` (`:790`, `:1300`, `:1726`, the name from `group_report_file` at `:872`-`:886`) plus the tracked
   `verification/RESULTS.json` from `publish_sweep` at `:1597`. Measured on the producer commit the sanctioned
   builder printed `tool tests: /home/shawn/RLLM/output/tools/harness-tests cases: 37`, and the same builder in the
   HEAD clone described below prints `tool tests: /tmp/g68room/output/tools/harness-tests cases: 37` — against the
   33 it printed at
   `69bc535`. Re-measured on these HEAD bytes in a clean clone (`/tmp/g68room`, `git rev-parse HEAD` =
   `1225caf9080056319a64143edf52336bc9054c6d`; `bash headless.sh` rc=0, `bash tools/build-tools.sh` rc=0, then
   `output/tools/harness sweep` rc=0): the sweep ended
   `64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4157 checks`, the delivered record printed
   `71 groups, 4157 checks`, and `output/checks/current-regression.json` holds 71 rows digesting
   `4f78fb07cd35740c28fba92a0bfd0583dc08f554fe16000e9809737eee35346f` — the same bytes the producer commit wrote —
   whose seven non-zero rows are `binary_flow`, `history_migration`, `field_flow`, `flags_flow`, `product_flow`,
   `apk_alignment`, `turn_background`, each one code 2. The instrument's location is part of the record: the same
   sweep in that room with the tools built to a lane out dir instead of the sanctioned `output/tools` printed
   `63 pass, 0 fail, 8 unavailable, 0 absent of 71 groups, 4145 checks`, because `apk_alignment` named the missing
   `output/tools/alignment` and `self_rebuild` the missing `output/tools/gate`.
   `output/tools/package --dry-run --destination /tmp/g68pub2` rc=1 then refused on a later input —
   `refusing: output/checks/apk.json absent` — which is the publisher accepting the record, since none of the four
   shapes `load_runs` refuses (`:2619`-`:2630`) fired. This tree's own `output/checks/current-regression.json` is
   still the 2026-11-12 file (sha `52976ff3c48f…`, untracked because `output/` is ignored): the sweeps above ran in
   clone rooms and wrote those rooms' records, and no sweep has been run in this tree since that file was written.
   What is owed on that route is therefore no longer a missing capability but the group-state gate: the record the
   HEAD sweep wrote carries seven code-2 rows, while `candidate_gate` (`tools/package.rs:1908`-`:1917`) returns Ok
   for a single failed row only when the publish is a candidate run and `exact_binary_flow` (`:1900`) matches that
   row — so seven is a refusal, not a tolerated gap. Split by the cause each leg names, those seven are:
   `binary_flow` (missing `output/pre-branch-controller`, `output/pre-auth-controller` and `output/published-controller`
   reference builds), `history_migration` (missing the `--previous-controller` reference at
   `output/previous-controller`), `field_flow`, `flags_flow` and `product_flow` (each missing the
   `published-controller` reference build), and `apk_alignment` plus `turn_background` (missing the companion APK
   `RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk` that `PUBLICATION-MANIFEST.json` names but a clean clone
   does not carry). Clearing the gate needs those owner-supplied reference builds and the companion APK present in
   the tree the sweep runs in. No publish has run: this tree's dry run printed
   no `would write:` line (`/tmp/g61-pub.txt`, count 0) and `sha256sum PUBLICATION-MANIFEST.json` still prints
   `35884d847b04eae66ca449588b97221a5f8bb0e74be0c91935b957e230156df2`, so no completion is claimed. `history_migration.py` 43 waits on its comparator, and that wait has its own
   run log: the room run of `output/harness history_migration` on the committed bytes printed
   `history_migration UNAVAILABLE (not executed, missing: path --previous-controller reference at
   output/previous-controller)` (`/tmp/g44room/group-hm.log:1`) with `summary: 0 pass, 0 fail, 1 unavailable of 71
   groups` (`:2`) at rc=0, and no build or package verb produces that path —
   `grep -c previous-controller headless.sh package.sh desktop.sh tools/*.sh` counts **0** matching lines. Of the
   three, `live_web.py` 71 is the one with no measured stop, and the measured facts about it are these: the runner
   prefers its built twin (`group_command` at `tools/harness.rs:1150`-`:1159` takes the `.py` only when the twin
   binary is absent), its group is an `OPTIONAL` row at `:38` refused before any leg runs — the refusal is `classify_requested_group`
   returning `DeclaredNotExecutable { kind: "optional group", reason: *reason }` at `:1895`-`:1896` — and the records of it
   this document measured are its manifest pin and the census line above — retiring it turns on that pin, and this document measured no leg that stops it: that is an absence of measurement
   in the artifacts named here, not a measured property of the leg, and no missing capability was measured for it. That last sentence covers the two `verification/` legs only;
   `tools/package.py` keeps the role the product declares for it (`TOOL_PYTHON` at `tools/package.rs:2186`-`:2191`)
   until a native publish runs. All three files are still tracked and still present (`ls -l verification/*.py
   tools/package.py` lists all three): the retirement is measured as not performed, and nothing here claims it has
   been scheduled.

---

## Addendum, 2026-11-14 — measured on `1225caf`, and one correction to this document

### The correction: group counts must be read from a clean HEAD room

A lane room that already carries that lane's earlier-round bytes is not a baseline. Every "unchanged"
count in the lane reports below was read in the lane's own room. The clean measurement is a fresh clone of
`1225caf` (`/tmp/g80room`, `bash tools/build-tools.sh`, then `output/tools/harness <group>`), transcript
`/tmp/baselineHead.log`:

| group | live checks at `1225caf` |
|---|---|
| `http_framing` | 189 |
| `network_feedback` | 41 |
| `response_fields` | 45 |
| `mcp_catalog` | 30 |
| `mcp_catalog_events` | 145 |
| `mcp_continuation` | 32 |
| `mcp_history` | 21 |
| `rrsi_loop` | 23 |
| `proposal_feedback` | 36 |
| `proposal_recovery` | 249 |

So the deltas the lanes actually carry are `http_framing` 189→192 (+3), `mcp_catalog` 30→38 (+8),
`mcp_catalog_events` 145→148 (+3) for lane G; `rrsi_loop` 23→34 (+11) for lane B; `proposal_feedback`
36→53 (+17) and `proposal_recovery` 249→277 (+28) for lane E. `network_feedback`, `response_fields`,
`mcp_continuation` and `mcp_history` are unchanged at 41, 45, 32, 21 — that part of the lane claims
reproduces. The line `recorded in verification/RESULTS.json: 71 groups, 4248 checks` that the runner
prints before a run is a pre-run read of the committed record, not this run's count.

### Closed: the phone chose its own background-turn lease (`689994a`)

`Activity.turnLease` used to clamp the window in Java and, when the core call threw, return the
*requested* timeout; the wake lock could then outlive or strand a turn on a number the core never
agreed to. The committed bytes ask the core (`TurnService.leaseOf` → `Core.call({"op":"turn_clock",
"window_ms":…})` → `lease_ms`), leave `turn_lease` in `core/lib.rs` as the only clamp, route a lease
failure through `turnProtectionLost` (notice `Background protection was lost. The turn still runs.`,
`notice_error=true`) without ever calling `TurnService.begin`, and make `onStartCommand` refuse a start
whose `lease` extra is absent before `startForeground` and before `wake.acquire`. Five paths,
+209/−53; the five worktree digests equal the digests in the witness room byte-for-byte.

Witnessed twice: fresh clone of `1225caf` + `git apply /tmp/laneH2b.patch` → `Rust cases passed:
885/885; isolated application cases: 58`, 72 twins, `twin helper tests … cases: 44`,
`harness-tests cases: 37` unchanged (`/tmp/laneH2-verify.log`); the same gate re-run in the main tree on
the committed bytes → `885/885; isolated application cases: 58`, and the two added cases named singly —
`checks::turn_clock_clamps_the_lease_and_formats_elapsed_time`,
`application::checks::turn_protection_notice_keeps_the_running_model_turn`. The artifact is read, not
assumed: the `turn_background` twin built from these bytes against the APK built from these same Java
digests exits 0 with `"passed": 29` and the check list carries
`a core miss supplies no lease and does not substitute the requested timeout` (`/tmp/h2-twin.json`).
The audit seat returned ACCEPT with R1, F03, F04, F05 CLOSED, on its own reading of the packaged dex
(`turnLease` has no catch table and invokes `TurnService.leaseOf` at 0009; fresh `onStartCommand` holds
the ticket check at 0027-003c and the absent-extra refusal at 003f-0054 before `startForeground` at
0097/009b and `wake.acquire` at 00b8; `TurnService.onTaskRemoved` is `stopSelf` + `return-void`).
Not claimed: no device install or upgrade, no measured main-looper serialisation, no real
background-start refusal, dataSync budget, wake-lock hold or notification delivery. The two lab packages
(1794667655, 1794668027) are unsigned and are not the deliverable.

### Seats that refused, and what they refused on

| lane | seat | open after adjudication |
|---|---|---|
| E (unknown-tool markup) | REJECT | F03 argument-form grammar, F06 the two-field rule, F08 U+200B — repair lane E4 running in `/tmp/g74room` |
| G (MCP/http) | REJECT (`/tmp/mm-laneG41b.out`) | F01, F02, F04, F07, F08 plus new notes: the 401 branch reads the status before the framed body, so a 401 whose body is not the framed payload is accepted; `plan` re-normalises the carried key and its idempotence is asserted, not shown; a foreign root URI returns the whole product set and `PRODUCT_ROOTS` is a process global; no route-level no-socket witness — repair lane G3 running in `/tmp/g75room` |
| B (RRSI pool scope) | REJECT (`/tmp/mm-laneB42.out`) | F06 test-name mismatch, F07 `append_file` over-claim, F08 `scope_names_its_row` overstates, F09 the assertion behind `pool_scopes_nonempty_roles_differ` is not its report line — repair lane B4 running in `/tmp/g78room` |
| H (Android lease) | ACCEPT | committed as `689994a` |

Nothing from E, G or B is committed; their bytes sit in their rooms until a seat accepts them.

### The APK build now has a run log (closing F9)

`/tmp/g50-chain.log:218`-`:236` is the witness: `apk_rc=0` at `:225`, then the delivered
`output/checks/apk.json` beginning `{ "version": 1794270238, "path":
"/tmp/g50room/output/2026-11-14__01-30-59Z/RLLM.apk", "bytes": 7837714, "sha256":
"a0254af2048e91eee1dcbcc39b68b4b37e04971dac4830d3ff6a0f75e9eed4a0"`. That is the signed route, on the
committed bytes of that chain. The test package the owner installs still has to be built above their
installed version code `1794275497` with the retained identity
`ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5`.

### Python retirement (lane K) — measured, not yet committed

`/tmp/laneK.patch` (34,729 B, sha256 `46ed9180ecd4f59471ae112b451c62f5bbfb70eb40ee3ce964e39fae23b43e72`,
base `1225caf`) deletes `verification/history_migration.py` (43 lines, 7 checks) and
`verification/live_web.py` (71 lines, 9 checks), removes the `python3` fallback so a missing twin prints
`runner could not start the group: missing twin binary <path>` and ends `FAIL`, sets
`VERIFICATION_PY_FILE_CEILING`/`VERIFICATION_PY_LINE_CEILING` to 0/0, and adds three harness cases. Check
parity was measured by the coordinator: the deleted `.py` files carry exactly 7 and 9 `check(` names and
the surviving `.rs` twins contain all 7 and all 9. What the patch does **not** fix: `docs/AUDIT.md` still
points a reader at the deleted files — `:896`, `:901`, `:910`, `:915`, `:920`, `:1045` and, at `:2456`,
"Optional live verification is available as `python3 verification/live_web.py --live`" — a sentence that
names a file the patch deletes. That reference is carried as an open item for the lane, not fixed under
its hands.
