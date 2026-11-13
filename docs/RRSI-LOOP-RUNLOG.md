# Refinement-loop run log

Transcripts of product runs, not readings of source. Each section is one Gap evidence bar from
`docs/RLLM-recursion-gap-report.md` §4, made on committed bytes inside a `git archive` room.

## Gap 2 — an RRSI run of three rounds on committed bytes — 2026-11-12T23:00Z, commit `b8b0e71`

The bar: *an RRSI run of at least three rounds showing the budget shrinking; at least one rejected
leaking candidate; a noise-floor decision; a pruned component; the incumbent as a worktree commit; a
held-out score.* Driver `/tmp/g2drive.py`, transcript `/tmp/g2drive-run3.log`.

### The room

```
git archive HEAD | tar -x -C /tmp/g2drive-room     # HEAD b8b0e71; a room carries no .git and no output/
ln -s /home/shawn/RLLM/tooling /tmp/g2drive-room/tooling
mkdir -p output/tools output/headless
bash headless.sh                                   # headless_rc=0, product_warning_lines=3
```

`output/headless/RLLM` is driven over `--jsonl` with `XDG_DATA_HOME` pointed inside the room and the
event `root` set to the room, so every file named below is a file the product wrote in the room. A git
workspace is planted in the room the way the delivered group does it (hand-written objects), seed commit
`65103240f6e7fc83f18720f836f0ba5a925e5bae`. The model is a controlled fixture endpoint; the agent carries
`instructions: INCUMBENT RULES`, so the incumbent body is 15 bytes long.

`product_warning_lines` is a `grep -c '^warning:'` count over the build log. The value 3 is two dead-code
warnings (`function historical_section is never used` at `RLLM/core/rrsi.rs:1538:15`, `function
require_worktree_root is never used` at `RLLM/core/rrsi.rs:1881:15`) plus rustc's own summary line
`warning: 2 warnings emitted`.

### The run, and the budget shrinking in the prompt the model actually receives

`refine` with `task='Drive three rounds.'`, `response_check_method='json_fields'`,
`response_checks='{"/a":"","/b":""}'`, `refinement_rounds='3'`, `evaluation_task='Held out drive task.'`,
`evaluation_method='phrases'`, `evaluation_checks='HELD-TOKEN'`. Each budget sentence below is quoted out
of the request the product sent over the model wire for that round:

```
refine opened:  phase=baseline
baseline scored: phase=held_out
PROPOSE WIRE, ROUND 1: ["Edits remaining this round: 4. The annealed budget is 4"]
pool leg scored: phase=baseline
PROPOSE WIRE, ROUND 2: ["Edits remaining this round: 3. The annealed budget is 3"]
PROPOSE WIRE, ROUND 3: ["Edits remaining this round: 2. The annealed budget is 2"]
round 3 closed: phase=evaluation_base
    decision=The first round was retained because the second did not add passing checks while
             preserving earlier passes under comparable conditions.
```

The prompt for a fourth round never opened, because after three rounds `skills` was pruned and the budget
annealed again; the review evidence the product shows ends with:

```
EDIT BUDGET
Edits remaining this round: 1. The annealed budget is 1. Pruned: skills.
```

### The ledger the product wrote

`output/g2-xdg/rllm/00329c4cb258c19c2d653dc63188f55a7d68324eaf89b162077df90a09e8f0c4.jsonl` — 6 sealed
rows, each `revision` chained to the row before it through `prior_revision`:

| # | round | component | hypothesis | verdict | decision | accept_reason | edit_budget | noise_floor | cost before → after (known) | score before → after |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | 1 | prompt | `{"a":""}` | accepted | cost not recorded | score | 4 | 0 | 15 → 8 (false) | 0 → 1 |
| 1 | 1 | prompt | `fa1c1836…9e16` | rejected | **refused because the candidate contains a recorded task** | — | — | — | 15 → 50 (not recorded) | *no score fields* |
| 2 | 1 | skills | flat one | rejected | cost not recorded | score | 4 | 0 | 15 → 52 (false) | 0 → 0 |
| 3 | 2 | skills | flat two | rejected | rejected revision | rejected revision | 3 | 2 | 8 → 52 (false) | 1 → 0 |
| 4 | 2 | plugins | raise the flag | rejected | **refused because the gain 1 does not exceed the noise floor 2** | floor | 3 | 2 | 8 → 69 (true) | 1 → 2 |
| 5 | 3 | skills | flat four | rejected | **refused because the cost rose while the score did not** | cost rose | 2 | 2 | 8 → 60 (true) | 1 → 0 |

`budgets per round: {"1": [4], "2": [3], "3": [2]}` — one `edit_budget` value per round, which is the same
4/3/2 the three prompts above carried.

Row 1 is the leaking candidate. The reply body copied the task text, so the critic stamped
`leakage: task_text` and the row was written *before* scoring: it carries no `score_before` and no
`score_after` key at all, `verdict: rejected`, and `pool_status: refused:refused because the candidate
contains a recorded task`. Its hypothesis is stored as a digest (`fa1c1836…9e16`) because the leaking text
itself is not kept in the ledger.

Row 4 is the noise-floor decision: that candidate gained one passing check (1 → 2) while the floor measured
for this task and verifier pair was 2, so the gain was inside the floor and the acceptance reason recorded
is `floor`.

Row 5 is the cost arm, which only fires because the loop now measures an incumbent cost: the score did not
rise and the candidate body was longer than the incumbent body, so the row records `cost_known: true`,
`cost_before: 8`, `cost_after: 60` and reason `cost rose`.

### Pruned component

`skills` was tried in rounds 1, 2 and 3 without a strictly positive gain, so it left the live component
set: `final state pruned_components=["skills"]`, `skills rows (rounds): [1, 2, 3]`, and the review evidence
prints `Pruned: skills.` in the budget line quoted above.

### The incumbent as a worktree commit

```
incumbent: ledger commit=e28d8f177ed8c616df9c23003af4df7d2b174a72
           refs/heads/rllm-evolve=e28d8f177ed8c616df9c23003af4df7d2b174a72
           .git/worktrees/c0/HEAD=e28d8f177ed8c616df9c23003af4df7d2b174a72
worktree file .rllm-worktrees/c0/rllm-harness.txt="{\"a\":\"\"}"
refs/heads/main=65103240f6e7fc83f18720f836f0ba5a925e5bae seed=65103240f6e7fc83f18720f836f0ba5a925e5bae (unchanged: True)
```

The accepted row's `commit`, the `rllm-evolve` ref and the linked worktree's `HEAD` are the same 40-hex
commit; the file inside that worktree holds the accepted candidate body `{"a":""}`; and `main` still points
at the seed commit, so the loop committed on its own branch and did not move the base.

### A held-out score

Rows 0 and 4 carry the four held-out keys:

```
held_out_known=true held_out_passed=1
held_out_task_key=094e93491d4cd8c1887301862ab12fd3d975ab20088237196a6a68b9995a2bbb
held_out_verifier=548b4612f8042dcde80994e965e86e4661b9f63b7a60c723a505c0864eed97a4
separate evaluation: base passed=1 candidate passed=0; in-round accepted score_after=2
```

The driver recomputes the two digests itself — `held_out_task_key` is SHA-256 over the canonical JSON of
`Held out drive task.` and `held_out_verifier` over `["HELD-TOKEN"]` — and they match what the product
stored. The separate evaluation (original instructions 1/1 phrases, retained instructions 0/1) is a
different measurement from the in-round score of 2 and is not recorded on those keys.

### The delivered group over the same bytes, in the same room

```
bash tools/build-harness.sh output/harness   # harness_build_rc=0, harness_build_warning_lines=0
output/harness rrsi_loop                     # summary: 1 pass, 0 fail, 0 unavailable of 71 groups
                                             # group_rc=0
```

The product wrote nothing to stderr (`product stderr bytes: 0`), and the settled run left one sealed entry
in `output/g2-xdg/rllm/experience.jsonl`, which is what the next section follows.

### What this log does and does not claim

- Scores come from a controlled fixture endpoint. What is measured is that the loop records a shrinking
  budget, refuses a leaking candidate before scoring it, states a noise-floor decision, prunes a component
  it names, commits the incumbent as a git worktree commit without moving `main`, and stores a held-out
  score — not that any model improved.
- Round 1's accepted candidate is the pool candidate `{"a":""}` (`pool_status: admissible`, and the row
  names its `branch` as `rllm-evolve`). Rounds 2 and 3 produced refusals only, which is why the run ends
  with the first round retained rather than a further gain.
- Nothing above is the driver's arithmetic: each value is printed from the product's request wire, its
  ledger file, its review evidence, or git files the product created.

## Gap 3 — task t+1 reading the library entry written by task t — 2026-11-12T23:13Z, commit `b8b0e71`

The bar: *task t+1 visibly reading the library entry written by task t.* Driver `/tmp/g3drive.py`,
transcript `/tmp/g3drive-run7.log`. The room is built the same way (`git archive HEAD` at `b8b0e71`,
`bash headless.sh` → `headless_rc=0`, `product_warning_lines=3` — the same two dead-code warnings plus
rustc's summary line explained above), with a planted git workspace seeded at
`65103240f6e7fc83f18720f836f0ba5a925e5bae`. **Task t and task t+1 are two separate product processes**,
so the second one can only see the first one's entry by reading it off disk.

`output/g3-xdg/rllm/experience.jsonl`: `exists before any run: False`.

### Task t, and the entry it wrote

`refine` with `task='Pool commit task.'`, `response_check_method='json_fields'`,
`response_checks='{"/flag":""}'`, `refinement_rounds='1'`,
`evaluation_task='Held out pool task.'`, `evaluation_method='phrases'`, `evaluation_checks='HELD-TOKEN'`.
It closed `phase=review` and sealed one edit-ledger row:

```
ledger c384ddd896c684ead9a425385d3b7e0cf5718d2297eb1321fc41a84c4f72726c.jsonl rows=1
row 0 round=1 component=prompt hypothesis={"flag":""} verdict=accepted pool_status=admissible
        accept_reason=score edit_budget=4 revision=68b856d6bb8b2cb7b186d9118f245b798d66bd4e…
```

Settling appended the library entry to `experience.jsonl` — one line, keyed by that task:

```
kind experience   schema library-1   task_complete true   falsified []   omitted_falsified 0
key            27c0edc8248038524823e4e9476b068a676b35b1a3bacbddf65be769c6b3a3be
revision       665268c8fa1bb9a4064f62da41f129a7d9becc6c45146b262c147ac935f89ecd
harness        {"flag":""}
harness_digest 65da7eef803ad2c44d556d4f1b8cde0495b5a6a054dce363354f484e4eb82c2a
run_digest     c384ddd896c684ead9a425385d3b7e0cf5718d2297eb1321fc41a84c4f72726c
task           "Pool commit task."
untried        control flow, config, context management, tools, skills, memory, subagents
```

`run_digest` is the same digest as the edit-ledger file name above, so the entry names the run that wrote
it.

### Task t+1: the same task text, a fresh process, and the prompt it was sent

A second `output/headless/RLLM` process, `refinement_rounds='1'`, same task and checks. The library was
never handed to it — it is on disk in that room and nothing else changed. Its propose request
(`bytes=9799`) carried, verbatim:

```
HISTORICAL EXPERIENCE
These records are historical experience from earlier tasks. They do not outrank the current task, its permissions, or the base harness.
- historical entry · indexed key matches this task · key 27c0edc8248038524823e4e9476b068a676b35b1a3bacbddf65be769c6b3a3be · revision 665268c8fa1bb9a4064f62da41f129a7d9becc6c45146b262c147ac935f89ecd
  Adopted harness (historical) · digest 65da7eef803ad2c44d556d4f1b8cde0495b5a6a054dce363354f484e4eb82c2a: {"flag":""}
  Evidence lesson: Lessons the sealed edit ledger supports: no hypothesis was falsified. Components with no trial in this run: control flow, config, context management, tools, skills, memory, subagents.
  Falsified hypotheses: none
  Untried components: control flow, config, context management, tools, skills, memory, subagents
```

The key, the revision and the harness digest in that block are byte-for-byte the values in the line the
first process wrote. The relation sentence says *matches this task* because the index is derived from the
task text, and the header states that the record does not outrank the current task.

That run did not settle: the fixture returned the identical body, the loop refused it as no new value, and
the run closed `phase=propose` with `library_revision=None`. It wrote no library row, which is the honest
outcome — a refused candidate does not extend the library.

### The same entry read by a run with a different key

A third run in the room, `task='A different bounded task.'`, `response_checks='{"/flag":"","/x":""}'`. Its
propose wire (`bytes=9646`) shows the same stored entry, differently related:

```
- historical entry · indexed key is from another task · key 27c0edc8248038524823e4e9476b068a676b35b1a3bacbddf65be769c6b3a3be · revision 665268c8fa1bb9a4064f62da41f129a7d9becc6c45146b262c147ac935f89ecd
  Adopted harness (historical) · digest 65da7eef803ad2c44d556d4f1b8cde0495b5a6a054dce363354f484e4eb82c2a: {"flag":""}
```

That run closed `phase=review` and appended **its own** entry under its own key:

```
row 0 key=27c0edc8… revision=665268c8… harness_digest=65da7eef… run_digest=c384ddd8…72726c task="Pool commit task."
row 1 key=876d67fb48697070c03fcc021fdcfe007c1ef92752aeb977b2fabf9ff2a2cb29
      revision=35dc5a0aeb8f950170034aa248ebb6027521b1e49822557adeaf353c482b6cd1
      harness_digest=b7c73e5793a5339476bf3a27da0723dd175482bbd2db71f1045d70fef7e5574a
      run_digest=adb59329076310267bc262205d453f57f1994094da51f5d6bdc4a855313a0a0d task="A different bounded task."
```

Row 0 is unchanged in bytes after the second settle — the library grew by append, and each entry keeps its
own `key`, `revision` and `run_digest`.

### Bars, and what is not claimed

```
bars passed 4/4     (task t wrote an entry; task t+1's wire carried that key, revision and harness digest
                     as "indexed key matches this task"; a differently-keyed run shows the same entry as
                     "indexed key is from another task"; the library grew by append with row 0
                     byte-identical)
product stderr bytes: 0 for both product processes
```

Scores again come from a controlled fixture endpoint. What is measured is the plumbing: a settled run
writes one line under a task-derived key, a later process reads that line off disk and puts its key,
revision and harness digest into the model's prompt with the relation stated, and a run under a different
key is told the record is from another task rather than being silently preferred.

### The delivered group over these bytes, in the same room

```
bash tools/build-harness.sh output/harness   # harness_build_rc=0, harness_build_warning_lines=0
output/harness rrsi_loop                     # rrsi_loop PASS (exit 0, 1s, 15 checks)
                                             # summary: 1 pass, 0 fail, 0 unavailable of 71 groups
```

## Gap 2 again, on the bytes this document is committed against — 2026-11-13T13:14Z, commit `095dc334`

The sections above at `b8b0e71` were made by a python driver in `/tmp` because the delivered group printed
nothing. This one is made **by the delivered group itself**: every value below is a line the group printed to
its own stdout, which `output/harness` copied into `output/checks/rrsi_loop.log`. No driver re-read an artifact
to quote it here.

### The room

`git archive` of `095dc3343be6982094dc2e8a5a0db8926d8b9631` into `/tmp/g23room` (no `.git`, no `output/`, only
`history_migration.py` and `live_web.py` left in `verification/`), `ln -s /home/shawn/RLLM/tooling` for the
client toolchain, then in the room: `bash headless.sh` → **`headless_rc=0`, 0 warning lines**;
`bash tools/build-verification.sh .` → **`verify_rc=0`**, `twins=72`, `twin helper tests … cases: 44`;
`bash tools/build-harness.sh` → **`harness_build_rc=0`** (the only verb that produces the room's
`output/harness` — my first attempt at this room ran without it and the runner died with
`output/harness: No such file or directory`, `runner_rc=127`, which is a missing input in my instrument, not a
product failure); then `output/harness rrsi_loop` → **`rrsi_loop PASS (exit 0, 1s, 18 checks)`**,
`output/checks/rrsi-loop.json` → `count 18, failed []`, `checks` carrying all 18 names, `scope` 2,277 characters.
Artifacts kept: `/tmp/g23room-out/rrsi_loop.log` (5,903 bytes), `/tmp/g23room-out/rrsi-loop.json` (5,782 bytes),
`/tmp/g23room-out/runner.log`.

### The six bars, as this run printed them

| bar the gap report asks for | the value the group printed, verbatim |
|---|---|
| annealed edit budget, round by round | `evidence anneal propose round 1 Edits remaining this round: 4. The annealed budget is 4.` → `anneal propose round 2 … 3. … is 3.` → `drive propose round 1/2/3` with `4.` / `3.` / `2.`, and the resumed request `anneal resumed propose Edits remaining this round: 1. The annealed budget is 3.` |
| leakage critic before scoring | the arm this run passes is the check `a re-proposed skills hypothesis is refused by name for round 1`; the drive run's round-1 rows print `noise_floor 0` (`drive ledger row 0 round 1 component prompt noise_floor 0`, `row 2 round 1 component skills noise_floor 0`). The group does not print the leak arm's per-row score fields, so the value-level leak evidence stays the `b8b0e71` section above (hypothesis stored as a digest, refusal recorded before scoring) |
| noise-adjusted floor | `drive ledger row 0 round 1 component prompt noise_floor 0`, `row 2 round 1 component skills noise_floor 0`, then `row 3 round 2 … noise_floor 2`, `row 4 round 2 component plugins noise_floor 2`, `row 5 round 3 component skills noise_floor 2`; the anneal and quiet runs stayed at floor 0 throughout |
| structural pruning | `evidence drive settled pruned_components ["skills"]`, with the negative arm printed too: `evidence flat skills settled pruned_components is absent` |
| the incumbent as a git commit | `evidence drive ledger commit 868dce24c1df5f5640c4474e81e19f8b01cb9f30 refs/heads/rllm-evolve 868dce24c1df5f5640c4474e81e19f8b01cb9f30 worktree HEAD 868dce24c1df5f5640c4474e81e19f8b01cb9f30 refs/heads/main 65103240f6e7fc83f18720f836f0ba5a925e5bae planted seed 65103240f6e7fc83f18720f836f0ba5a925e5bae main equals planted seed is present` — the same three ways for the pool arm at `97b4a1b075ec451499212f41564954153bcbc0c7` |
| held-out / separate evaluation | `evidence pool separate evaluation base passed 1 separate evaluation candidate passed 0 in-round score_after 1`, and five held-out rows, e.g. `drive held-out row 5 round 3 component skills held_out_known true held_out_passed 1 held_out_task_key 094e93491d4cd8c1887301862ab12fd3d975ab20088237196a6a68b9995a2bbb held_out_verifier 548b4612f8042dcde80994e965e86e4661b9f63b7a60c723a505c0864eed97a4` |

Of the 18 names in the report, the fifteen that were already there at `b8b0e71` all still pass, and the three
delivered since then are `a later refine task reads the earlier task's sealed experience entry from the propose
prompt`, `a refine task whose library holds no sealed row shows no historical experience section`, and
`the run log prints the annealed budgets, the evolve commit three ways, the noise floors, the pruned set, the
held-out digests and the sealed library row` — the last one is what makes this log self-certifying: if any of
those prints degrades to `absent`, the group goes red instead of quietly printing less.

### What this run does not claim

The pool arm printed its row decisions (`pool ledger commit …`, `pool separate evaluation …`) but the pool's
per-candidate decisions are still not check targets, so they stay on the owed list. No OOD split is driven. The
log check certifies that each printed line carries its label and expected value; it does not re-derive every
printed value from a second copy of the artifact (see `project-progress.md`, "A print that says `absent` used to
be green").

## Gap 3 again on these bytes — same room, same run, 2026-11-13T13:14Z, commit `095dc334`

The decisive bar of the gap report is *"task t+1 visibly reading the library entry written by task t"*. At
`b8b0e71` that was witnessed by a driver comparing two product processes. Here it is inside the delivered
group, and the library row is printed rather than quoted by a driver:

```
evidence library before a sealed row key absent revision absent harness_digest absent run_digest absent
evidence earlier sealed library row key b19a20739a344697ec81bae2cb21d8c86d474eef70dfff0fcf971dc0cfc33a6a
             revision 4fb634b436cc678dbcb917a3e453d73c817096a666f4a7f2acd71a6bd087c6fa
             harness_digest 79a298286981294143558b1d13f30dbb8b1d64e9b6c643399bdc0bc7c6c89645
             run_digest c8a52ebe2857fab0f3ff6a10da4c8fb63f458f963f1ccc7117d642de457d0926
```

The same run passes `a later refine task reads the earlier task's sealed experience entry from the propose
prompt` — the task-t+1 propose wire carries task t's key, revision and harness digest under
`HISTORICAL EXPERIENCE`, keyed to the earlier task — and `a refine task whose library holds no sealed row shows
no historical experience section`, which is the negative arm that stops a vacuous pass. `a tampered library
entry is refused and its bytes stay unchanged` covers the write side. What remains un-witnessed for Gap 3 is
the layer the sealed-row vocabulary does not carry at all: deliverables, reflection notes, and
invariant/domain scoping (`docs/RLLM-gap-status-2026-11-13.md`, Gap 3 row).

## Gap 2 #11 and #12 — the k-candidate pool, witnessed by the delivered group

Made on the committed lane bytes: commit `065efd31121593ab51bd89a151dd5f7df4ae72c2` carries
`verification/rrsi_loop.rs` whose `sha256` is `c6994ce5b53eb4abdd638037adacfc08af91cd407f6096c3fd290a180302bc50` —
`git cat-file -p 065efd3:verification/rrsi_loop.rs | sha256sum` — and the witness room verified that same
sha256 on the file it built (`room_lane_sha=c6994ce5b53eb4abdd638037adacfc08af91cd407f6096c3fd290a180302bc50` in `/tmp/g30room.out`). The room is a `git archive` extract of commit `192d6aa2f358b1c433dd6c52ebbcc892ec6db85e` — that id is what `/tmp/g30room-out/commit.txt` records —
with this lane and `closure_ledger.json` copied over it (`/tmp/g30room.sh`); `project-progress.md` was never overlaid, so the room holds 192d6aa2's own committed copy of it.
`git diff --name-only 192d6aa2f358b1c433dd6c52ebbcc892ec6db85e 065efd31121593ab51bd89a151dd5f7df4ae72c2`
lists exactly `closure_ledger.json`, `project-progress.md`, `verification/rrsi_loop.rs`. Measured against the
commit tree rather than deduced, path by path: `find /tmp/g30room -type f -not -path '*/output/*'` lists 354
paths and `git ls-tree -r --name-only 065efd3…` lists 354, and `comm` over the two sorted lists is empty in
both directions — identical path sets. Then every one of those 354 room files was hashed (`git hash-object`)
against its committed blob: **352 match in both hash kinds and exactly two differ in both**, and their presence in the room
is measured, not called "drafts": `project-progress.md` in the room is 192d6aa2's committed copy (room
`2bdc3c5ced4f…` = that commit's blob digest), which `065efd3` later rewrote to `00ca7efedf97…`; `closure_ledger.json`
in the room is the working-tree copy the script overlaid on 192d6aa2's copy (`5e65deeeafce…`), at room
`7eea146b2c71…` against committed `3bb2d97b9d0c…`. `verification/rrsi_loop.rs` is on the identical
side (`c6994ce5b53e…` on both). The room measures 455 files in all: 101 of them under `output/` and one `tooling` symlink; no file under `output/` is in the commit tree — `git ls-tree -r --name-only 065efd3 | grep -c '^output/'` = 0 (witness `/tmp/gapmissing.txt`) — while that room's own `output/` holds 237 generated files.
That join was re-measured on 2026-11-13 by `/tmp/g30joinnow.py` — artifact `/tmp/g30join-now.txt` (`1681` bytes, sha256 `b64bbf5c2a12…`) — and the published numbers hold. The room's non-`output/` path set is identical to `192d6aa2`'s tree and to `065efd3`'s (`only_room=[] only_tree=[]` against each, `tree_paths(base 192d6aa)=354 tree_paths(lane 065efd3)=354 room_nonoutput_paths=354`). Against the room's own base `192d6aa2`: `compared=354 identical=352 differing=2`, the two being `closure_ledger.json` (room blob `d49f323cf53c`, base `a00e23c9c70e`) and `verification/rrsi_loop.rs` (room blob `955a094d12c3`, which is `065efd3`'s own blob, base `6964c848c3cd`) — that pair *is* the overlay. Against the lane commit `065efd3`, the join this section reports: `compared=354 identical=352 differing=2`, the two being the documents named above. One correction of kind: the 12-hex digests quoted in this paragraph are **sha256 of file bytes**, not `git hash-object` blob ids — the artifact prints both kinds side by side (`project-progress.md` room blob `1704d79d9c93` / room sha256 `2bdc3c5ced4f`; `closure_ledger.json` room sha256 `7eea146b2c71` against committed `3bb2d97b9d0c`). Byte equality is established by both kinds; only the digest label was wrong. So
the room is **not** a byte-copy of `065efd3`: `project-progress.md` is there exactly as `192d6aa2`
committed it, and `closure_ledger.json` is the working-tree copy the script overlaid. Whether those two differing files could have entered this gate was closed by enumeration. The verb-to-root map this sweep used is: `headless.sh` -> `core/`, `tools/build-verification.sh` -> `verification/`, `tools/build-tools.sh` and `tools/build-harness.sh` -> `tools/` + `verification/` — 125 `core/*.rs`, 72 `verification/*.rs`, 9 `tools/*.rs` (`find <root> -maxdepth 1 -name '*.rs' | wc -l`). A source file can enter that compilation without being named by a verb only through an include directive, and `grep -rn "include_str!\|include_bytes!\|include!" --include='*.rs' core tools verification` finds exactly three sites: `core/desktop.rs:4` and `core/console.rs:2`, both `include!("lib.rs")`, and a string literal at `verification/self_rebuild.rs:1861` in the working tree (`:834` at `065efd3`) that becomes `include_str!("../src/edited.rs")` in a scratch crate that check builds at run time. None of the three names a claim document. The reader census over the same three roots (`grep -rln --include='*.rs' --include='*.py' --include='*.sh' <name> core tools verification`, `__pycache__` excluded by the include filters) is `closure_ledger` 0 files, `RRSI-LOOP-RUNLOG` 0 files, `RLLM-gap-status` 0 files, `project-progress.md` 2 files — `tools/package.rs:2674`, `:2911`, `:3054` and `tools/package.py:77` — and `package` is a verb this gate never ran. So inside the enumerated closure (the four verbs' roots, every file in them, and every include directive in those files) neither differing document can move a verdict; the closure is what these greps cover and nothing is claimed beyond it.

A byte-copy room, nothing overlaid. `/tmp/g39room.sh` runs `extract: git -C /home/shawn/RLLM archive 065efd31121593ab51bd89a151dd5f7df4ae72c2 | tar -x -C /tmp/g39room ; ln -s /home/shawn/RLLM/tooling /tmp/g39room/tooling` and copies no file over it (`tooling` is the only symlink); the transcript binds one run to one room (`run_pid=4146948 run_started=2026-11-13T16:12:44-05:00`, `room_has_git=no`). Byte identity is measured path by path, not argued: `git ls-tree -r --name-only 065efd31121593ab51bd89a151dd5f7df4ae72c2` lists 354 paths and `find /tmp/g39room -type f -not -path '*/output/*'` lists 360; every tree path is present, and hashing each room file with `git hash-object` against `git rev-parse 065efd31121593ab51bd89a151dd5f7df4ae72c2:<path>` gives 354 compared, 354 identical, 0 differing, 0 missing. The 6 non-`output/` files the room holds beyond the tree are the logs I redirected into its root (`harness.log`, `headless.log`, `rrsi_loop.log`, `sweep.log`, `tools.log`, `verify.log`); none is a compile input. Restricted to the roots the four verbs compile, 235 files, 235 of them byte-identical. The room holds 597 files in total: 237 built by the room itself under `output/`, 1 symlink, the rest the tree plus my logs. In that room: `bash headless.sh` → `headless_rc=0 warning_lines=0`, `headless_binary=present`; `bash tools/build-verification.sh .` → `verify_rc=0 twins=72 helper=cases: 44` (the `twins=72` counter counts the `twin: ` lines that script prints; the directory holds one twin binary per `verification/*.rs` plus `twin-helper-tests`); `bash tools/build-tools.sh output/tools` → `tools_rc=0 cases_lines=10 err_lines=0`; `bash tools/build-harness.sh` → `harness_build_rc=0` (`harness_binary=present sha256=ac42908e633f6a95`); and the group itself `output/harness rrsi_loop` → `rrsi_loop PASS (exit 0, 2s, 23 checks)` with the report that run wrote — count 23, `failed` empty, `passed` 23, scope string 4692 characters (`/tmp/g39room/output/checks/rrsi-loop.json`; the runner log is `/tmp/g39room/rrsi_loop.log`, and this room's transcript prints a traceback for its own inline report reader, which looked at `output/rrsi-loop.json` instead of the `output/checks/` path the runner writes). So on the committed bytes the compile-closure question does not arise: every committed tree path in that room is byte-identical to `065efd3`, and measured beyond those identical files: 360 non-`output/` files (the six redirected logs are the room-only ones), 237 files under `output/`, and one symlink. The same room's full sweep printed `sweep_rc=1 summary: 64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4154 checks` over 71 per-group rows, the one failing row being the pre-existing `history_migration` group (`history_migration FAIL (exit 101, 0s, absent checks)`) — because that reference is never produced on the measured route: the only *writes* of `output/previous-controller` in tracked source are test bodies at `tools/harness.rs:2488` and `:2509`, inside `#[cfg(test)]` which begins at `:1986`; the only other tracked references are the input declaration at `:56` and the default-path construction (`:1085` in the working tree, `:1077` in `065efd3`); and no build script in the measured set mentions the path (`grep -rn previous-controller tools/*.sh headless.sh package.sh desktop.sh` = 0 hits); `leg_source` reads `verification/<group>.rs` when that file exists and falls back to `.py` only otherwise, so for this group the input detector reads the **native** twin; the twin states its requirement in its own parse error (`verification/history_migration.rs:73`, `the following arguments are required: --previous-controller`) while the committed detector looked only for a script's `add_argument('--previous-controller'` declaration, so the requirement was invisible to it; the harness then hands the twin its default path (`tools/harness.rs:1085` in the working tree, `:1077` with the push at `:1079` in `065efd3`; the input is declared at `:56`) and the twin spawns that path as a controller binary and panics on the missing file — the room log carries the panic verbatim: `could not start /tmp/g39room/output/previous-controller: No such file or directory (os error 2)` (`/tmp/g39room/output/checks/history_migration.log`); the panic site is `verification/twin/mod.rs:243-249`, `panic!("could not start {}: {error}", binary.display())` inside `spawn_at`. The delivered record disagrees with that red, and the disagreement is dated rather than argued: the record's `connected.unavailable` entry {"name": "history_migration", "code": null, "missing": ["--previous-controller reference at output/previous-controller"], "recorded_as": 0} (and its `checks` entry, which adds "history_migration was not executed because a development input is missing: --previous-controller reference at output/previous-controller.") was published at `0eb0cb1` (2026-11-12 19:38Z) and the native twin landed later at `54c1f1c` (2026-11-13 10:22Z); `git merge-base --is-ancestor 0eb0cb1 54c1f1c` → true, so that entry describes the python-leg era. A working-tree harness fix now reports that group `UNAVAILABLE` with its missing-input reason instead of failing: on the same lane bytes the committed harness sweeps `64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4156 checks` and the fixed harness sweeps `64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4156 checks` (`/tmp/g32room/output-sweep.log`, `/tmp/g36sweep.log`, `/tmp/hmfix.out`); keyed by name, verdict and check count, exactly one row of those two rooms differs — `history_migration FAIL (exit 101, 0s, absent checks)` against `history_migration UNAVAILABLE (not executed, missing: path --previous-controller reference at output/previous-controller)` — and both rooms' per-group sums are 4156. The publication gate is measured on the same bytes, not assumed: `commit_sweep` (`065efd3:tools/harness.rs:1616`) returns before `publish_sweep` when any row failed and prints `delivered record left untouched: a group failed`, which is what this room printed (`delivered record left untouched: a group failed`); its `verification/RESULTS.json` is byte-unchanged, before and after the sweep (`results_before=1450f95863f9cc05b3c61e95ce7941d7f3154855 results_after=1450f95863f9cc05b3c61e95ce7941d7f3154855 results_committed=1450f95863f9cc05b3c61e95ce7941d7f3154855`). The zero-failure room on the same lane bytes writes its own record — `delivered record written: 71 groups, 4156 checks` (`/tmp/g36sweep.log`) — and its record's git blob id moves from the committed `1450f95863f9cc05b3c61e95ce7941d7f3154855` to `3887aaab73138408910de1461ba427a351f01dc7` — both are blob ids of `verification/RESULTS.json` (`git rev-parse <rev>:verification/RESULTS.json`), and their sha256s are different numbers (the committed file hashes to `e2f35542e44b…`), in a directory that is no git repository at all (`room_has_git=no` here, and `git -C /tmp/g36room rev-parse HEAD` → `fatal: not a git repository`). The sentence this document carried before — that publishing needs a git repository and a git-archive room has none — was wrong. The byte-copy room is also the arity witness this lane turns on: there `self_rebuild` is `self_rebuild PASS (exit 0, 1s, 9 checks)`, while the lane rooms report `self_rebuild PASS (exit 0, 2s, 11 checks)` — 4147 more checks in that group. Those 11 checks are that lane's count at those two room runs; the lane file now carries a twelfth check, `a review permit refuses a same-revision decoy commit and commits the tracked source tree above it`, and a a `git archive` room of `065efd3` with the three lane files copied over it (an overlay room, not a byte-copy room) runs the group green at `12 self-rebuild checks passed` (`/tmp/g42room-out/self_rebuild-baseline.log:18`), with the mutation arms that show the product's commit-tree guard is what makes that check pass recorded in `docs/RLLM-gap-status-2026-11-13.md`. Comparing all 71 per-group rows of the two rooms (`/tmp/g39-verdicts.txt` against `/tmp/g32room/output-sweep.log`), keyed by group name with exit code and elapsed seconds dropped, changes exactly one row, that one. The whole-tree sums differ by 2 checks (4154 here, 4156 there), which is that group's difference and nothing else. That room is not an overall green and is not claimed as one: the four build verbs and the group run these claims rest on return rc=0 (`headless_rc=0`, `verify_rc=0`, `tools_rc=0`, `harness_build_rc=0`, `runner_rc=0` with the group verdict above), and the whole-tree sweep there is `sweep_rc=1` — `64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4154 checks`. The room cited in earlier rounds as `/tmp/g35room`, and briefly as `/tmp/g38room` (whose transcript was truncated when a second launch opened the same output path), was rebuilt as `/tmp/g39room`; every figure in this paragraph is read out of `/tmp/g39room.out` and `/tmp/g39-verdicts.txt` by the script that rewrote this document, and it agrees with the earlier byte-copy runs on the byte join (354 compared, 354 identical) and on the sweep summary. The runner line `recorded in verification/RESULTS.json: 71 groups, 4248 checks` is a pre-run read of the delivered record, and the sweep's own total is 4154 checks — three counters, three scopes, as are the verify log's `twins=72` and the 73 files listed in `output/verification` (72 twins plus `twin-helper-tests`). `twins=72` is the room script's own summary field (`/tmp/g39room.sh:16`), which counts `^twin: ` lines in `/tmp/g39room/verify.log`; that count re-measures as 72 (`grep -c '^twin: ' /tmp/g39room/verify.log`), and the room transcript itself carries no `twin: ` lines (`grep -c '^twin: ' /tmp/g39room.out` = 0), only the summary line `verify_rc=0 twins=72 helper=cases: 44`. The 73 is `ls /tmp/g39room/output/verification | wc -l` = 73 (witness `/tmp/twinwitness.txt`).

Both rooms are reported because they witness different things: the transcript above is the witness for the lane as it stood *before* `065efd3` (its lane bytes and the working-tree ledger were overlaid on 192d6aa2's tree), and the byte-copy room is the witness for the committed bytes themselves, where nothing of mine is overlaid. The room ran its own
`bash headless.sh` → `headless_rc=0` with zero warning lines, its own
`bash tools/build-verification.sh .` → `verify_rc=0` (72 twins, `twin helper tests … cases: 44`),
its own `bash tools/build-tools.sh output/tools` → `tools_rc=0`, whose build log carries ten `cases:` lines (`/tmp/g30room/output/tools-build.log`), its own
`bash tools/build-harness.sh` → `harness_build_rc=0`, then
`output/harness rrsi_loop` → `runner_rc=0`:

```
rrsi_loop PASS (exit 0, 2s, 23 checks)
```

Report `['checks', 'count', 'failed', 'passed', 'scope']`, `count` 23, `failed` [],
`scope` 4692 chars.
The pool-line census, by command: `grep -c pool /tmp/g30room-out/rrsi_loop.log` = 18,
`grep -c '^evidence .*pool' …` = 12, `grep -c '^leg .*pool' …` = 6, nothing else. `verification/RESULTS.json` unchanged (`yes`): the room prints `results_before=1450f95863f9cc05b3c61e95ce7941d7f3154855 results_after=1450f95863f9cc05b3c61e95ce7941d7f3154855 results_committed=1450f95863f9cc05b3c61e95ce7941d7f3154855`. That 40-hex value is the **git blob id** of the file at `065efd3` (`git rev-parse 065efd3:verification/RESULTS.json`), not a content digest; the same bytes hash to sha256 `e2f35542e44b…`, which is a different number. In the shipped code the only call site of `publish_sweep` outside the test module (which begins at `tools/harness.rs:1978`) is `tools/harness.rs:1621`, inside `commit_sweep` (defined `tools/harness.rs:1616`); `commit_sweep` has exactly one caller outside that test module, at `tools/harness.rs:1732` inside `run_sweep` (defined `:1717`), and `run_sweep`'s only call site is the runner's `sweep` branch (`tools/harness.rs:1840`, call at `:1841`) — so in the shipped code the chain into that writer is `sweep` → `run_sweep` → `commit_sweep` → `publish_sweep`. Those numbers are `tools/harness.rs` as `065efd3` committed it (`git show 065efd3:tools/harness.rs`), the copy the witness rooms compiled; the working tree carries a later uncommitted edit to that file (the native required-input census), so the same symbols sit further down there. This lane ran `output/harness rrsi_loop`, which never enters that branch, so it never reached that writer; whether some other program could write the file is not claimed. The room transcript above is this claim's gate — every verb in it ran inside that room over the
bytes that `gate.room` compares path by path against the commit tree — 352 of 354 blobs identical, the two exceptions named above — so it is a claim about that room's bytes, not about the working tree, whose separate state is
recorded in the next sentences. An earlier in-tree attempt at the same gate came back
`tools_rc=1` — the transcript of that attempt is no longer on disk, so what is recorded here is the ledger
row's `gate.note`: `error[E0425]: cannot find function `room_handoff` in this scope -->
RLLM/verification/self_rebuild.rs:100:43`, another lane's half-written twin in the same tree — an attribution to that path, because which tree state produced the red was never measured, and it is a file this commit does not carry. That is why the gate is run in a room whose bytes are the ones
claimed: the red named `verification/self_rebuild.rs`, a file this commit does not carry. A later
`bash tools/build-tools.sh /tmp/toolsmain` run in the main tree — with that other lane's files still present —
finished `TOOLS-RC=0` with ten `cases:` lines and no `error[` line (`/tmp/toolsmain.log`), so the red did not survive
into a build of the then-current main tree; that run is a separate observation about the working tree, not
part of this gate, and which state of the concurrent writer produced the red was not measured.
The group log carries 37 `evidence` lines.

The pool values the group printed, unelided and verbatim from that room log — each of these is one
`evidence` line of that log, marker included. The log mentions `pool` on 18 lines: these twelve
evidence lines and the six `leg` lines that name the checks, so no line containing the literal token `pool`
is withheld — that census is a token census, not a semantic one, and says nothing about pool-shaped lines
that never use the word:

```
evidence pool ledger commit 9c671e6cc0498676a926c0c9f8df376b8deb7dc3 refs/heads/rllm-evolve 9c671e6cc0498676a926c0c9f8df376b8deb7dc3 worktree HEAD 9c671e6cc0498676a926c0c9f8df376b8deb7dc3 refs/heads/main 65103240f6e7fc83f18720f836f0ba5a925e5bae planted seed 65103240f6e7fc83f18720f836f0ba5a925e5bae main equals planted seed is present
evidence pool separate evaluation base passed 1 separate evaluation candidate passed 0 in-round score_after 1
evidence pool decision row 0 pool_status admissible score_before 0 score_after 1 commit c7a733ba7179e6098bd27cd3c38c67618e891452 verdict accepted run_digest 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 journal 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 evolve c7a733ba7179e6098bd27cd3c38c67618e891452
evidence pool decision row 1 pool_status refused:refused because the candidate contains a recorded task score_before absent score_after absent commit 234ee79ed76e2dafb17505f5ed9939665f844566 verdict rejected run_digest 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 journal 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 evolve c7a733ba7179e6098bd27cd3c38c67618e891452
evidence pool decision row 2 pool_status refused:the score did not rise score_before 0 score_after 0 commit 3f29f439e712ae69212ea1f2cc22dee2dc361412 verdict rejected run_digest 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 journal 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 evolve c7a733ba7179e6098bd27cd3c38c67618e891452
evidence pool decision rows journal 1d6dcdd6304cdbcde357ffee035c3ee841caea9af25839c97c3fb449002744e3 rows 3 stamped 3
evidence pool slot c0 row_commit c7a733ba7179e6098bd27cd3c38c67618e891452 worktree_head c7a733ba7179e6098bd27cd3c38c67618e891452 slot_git present harness_text {"a":""}
evidence pool slot c1 row_commit 234ee79ed76e2dafb17505f5ed9939665f844566 worktree_head 234ee79ed76e2dafb17505f5ed9939665f844566 slot_git present harness_text Copy the task Drive three rounds. into the harness
evidence pool slot c2 row_commit 3f29f439e712ae69212ea1f2cc22dee2dc361412 worktree_head 3f29f439e712ae69212ea1f2cc22dee2dc361412 slot_git present harness_text component: skills hypothesis: flat one flat body one
evidence pool parse task Count mismatch task. phase propose notice candidate pool does not contain the declared candidates digest absent ledger bytes 25093 to 25093 marks 4 to 4 task_run absent post run_digest absent
evidence pool parse task Count limit task. phase propose notice a round carries at most 3 candidates digest absent ledger bytes 25093 to 25093 marks 4 to 4 task_run absent post run_digest absent
evidence kept pool phase propose note incumbent kept because no candidate was admissible notice incumbent kept because no candidate was admissible composition INCUMBENT RULES incumbent INCUMBENT RULES kept present digest 737d6ba8b2b1304fe76ea125d7e9f23fe31de37f8363e39c868ac6cac4e9a2f2 statuses refused:refused because the candidate contains a recorded task,refused:refused because the candidate adds no change to component prompt,refused:refused because the candidate contains a recorded expected value evolve c7a733ba7179e6098bd27cd3c38c67618e891452 to c7a733ba7179e6098bd27cd3c38c67618e891452 harness {"a":""} to {"a":""} slot harness {"a":""} to Copy the task Keep the incumbent. into the harness
```

What those lines back, as check names in this group:

* `a three-candidate pool writes one ledger row per candidate and stamps each decision` — three round-1 pool
  rows in that run's own digest-named journal, every row stamping that journal's digest, one admissible row
  scoring 0 → 1 whose commit is simultaneously the `rllm-evolve` ref and `.git/worktrees/c0/HEAD`, one
  leaking refusal carrying **no** `score_before`/`score_after` fields, one `the score did not rise` refusal
  carrying 0 → 0, exactly two refusals.
* `each candidate of the driven pool has its own worktree whose HEAD is that candidate's row commit and whose
  harness copy is that candidate's body` — the three slot worktrees that round leaves behind, each with its
  own `.rllm-worktrees/cN/.git` file, `.git/worktrees/cN/HEAD` equal to that candidate's row commit, three
  pairwise-distinct commits, and that candidate's own body in the slot's `rllm-harness.txt`. This is the
  durable post-round state the product leaves (`commit_candidate` at `core/rrsi.rs:1969` takes the slot index,
  removes any stale slot, adds a detached worktree, writes the body; `core/harness.rs` calls `slots.keep()`);
  simultaneous occupancy during `evaluate_pool` is **not** claimed.
* `a declared candidate count that does not match its blocks is refused by name and writes no pool rows` and
  `a candidate count above three is refused by name and writes no pool rows` — the product's own sentences
  (`candidate pool does not contain the declared candidates`, `a round carries at most 3 candidates`) read out
  of that reply's returned state, with the root-wide pool-status mark count unmoved (`marks 4 to 4`), the
  root-wide ledger byte total unmoved (`25093 to 25093`), no task run in the returned state, and no 64-hex run
  digest either before the reply or in the state the reply returns — the log's own `evidence library before a sealed row key absent revision absent harness_digest absent run_digest absent` line and the two parse lines ending `post run_digest absent`.
* `a pool with no admissible candidate keeps the incumbent and the composition` — exactly three rows in that
  arm's own digest-named journal, each stamping that arm's `run_digest`, each with a `refused:` status (the
  lane reads those rows from the journal file and prints only the joined statuses and the digest, so the log
  line is a summary of that read, not the rows themselves); `incumbent_kept` true, which the log prints as
  `kept present` because this lane's `yes_no` renders true as `present`; the product's own round note; the
  composition byte-equal to the incumbent instructions read back out of product state; the `rllm-evolve` ref
  unmoved (`evolve c7a733ba… to c7a733ba…`); the room's own `rllm-harness.txt` present and unmoved
  (`harness {"a":""} to {"a":""}`); and the c0 slot copy of that file equal to one of the three values that check accepts by name.
* The last check reads this run's own log — its required-line list is `verification/rrsi_loop.rs:1409-1439` (`let has_pool_decisions = [` at `:1409`, the nine needle strings at `:1410-1418`, the array literal spans `:1409-1419` (closing `]` at `:1419`), then `.iter()` at `:1420` and `.all(|needle| lines.iter().any(|line| line.contains(needle)))` at `:1421`, and the journal, parse and slot conjuncts follow at `:1422-1439`, closing with `});` at `:1439`),
  which names `"pool slot c0 row_commit"`, `"pool slot c1 row_commit"`, `"pool slot c2 row_commit"` and
  `"kept pool phase propose note incumbent kept because no candidate was admissible"`, and whose journal
  predicate is `line.contains("pool decision rows journal") && line.ends_with("rows 3 stamped 3")` — it
  requires the three decision rows, the three slot lines with
  40-hex row commit and HEAD and `slot_git present`, the journal summary ending `rows 3 stamped 3`, both parse
  lines ending `post run_digest absent`, and the incumbent-kept note. A print that rots turns the gate red.

That the five pool checks are new in this commit is measured on the blobs rather than assumed from the report:
the five names as the room report prints them — `a three-candidate pool writes one ledger row per 
candidate and stamps each decision`, `each candidate of the driven pool has its own worktree whose HEAD 
is that candidate's row commit and whose harness copy is that candidate's body`, `a declared 
candidate count that does not match its blocks is refused by name and writes no pool rows`, `a candidate 
count above three is refused by name and writes no pool rows`, `a pool with no admissible candidate keeps 
the incumbent and the composition` — each occurs 0 times in `git show 065efd3^:verification/rrsi_loop.rs` and once in the added lines of `git show 065efd3 -- verification/rrsi_loop.rs`; the two other pool-mentioning checks in that file
(`a non-refine run carries no workspace and does not attempt the pool`, `the incumbent commit in
.rllm-worktrees is the evolve tip`) occur once in the parent blob and in no added line, i.e. they predate it.
The second of those is a check name, and the run-specific claim it stands for is the witnessed line above —
`evidence pool ledger commit 9c671e6c… refs/heads/rllm-evolve 9c671e6c… worktree HEAD 9c671e6c…` — i.e. the
equality is asserted for that run, not offered as an invariant of every checkout.
Two counts of that word, both measured: matching lines (`git show <rev>:… | grep -ci pool`) = 50 parent / 124 committed;
occurrences (`str.lower().count('pool')`) = 66 parent / 164 committed.

Falsifiability came from seven mutation batteries on uncommitted lane bytes — the seven logs end in six distinct restore digests:
`fd2d0799…`, `a3984bdc…`, `3d46e4a6…` twice, `da8a22b0…`, `15bc8ee8…`, `daa9f5a3…`. No battery ran on a committed lane state: the parent lane blob hashes to `a9c3e82df860b1af…` and the committed lane blob to `c6994ce5b53eb4ab…`, and none of the seven logs' six restore digests is either of them. What the last battery base differs from the committed file by is not reproducible from a git object — no git object holds that base. Seat 5 read that difference off the battery's own base copy and reported it as the SCOPE string plus one comment line, six changed lines because the SCOPE constant spans lines; that stands here as a seat attestation, not as a diff this witness can re-run. Re-census of those seven logs, re-parsed while the committed lane was the checked-out file (the seven logs end in six distinct restore digests, none of which is a committed blob, so the runs were made on uncommitted lane states): 58 `== arm` records — 45 ran `rc=101` and name the check they broke (31 print `named='<check>'`; the
two earliest batteries print the same fact as `panic: … panicked at verification/rrsi_loop.rs:<line>` plus
`last leg: leg <check>`), 10 are green, 2 print `== arm … SKIP landing hits=2`, and 1 fails to compile
Per log (`== arm` records / `-- landing` records): poolmut.out 7/7, poolmut2.out 9/9, poolmut3.out 17/15,
poolmut4.out 3/3, poolmut5.out 7/7, poolmut6.out 7/7, poolmut7.out 8/8 — 58 arms, 56 landings.
(`== arm r9 compile_rc=1 errs=2`). 56 of the 58 arm records have a corresponding `-- landing <id>: … sha=<12-hex>` record; the two that have none are the SKIP arms, which were never built. Each battery log
ends `RESTORE-SHA <sha> unchanged=yes`, where `unchanged` is computed by sha256 against that battery's
pre-mutation bytes. Of the 10 green arms, 6 are zero-change battery controls (`p0`, `p0`, `q0`, `s0`, `t0`,
`v0`) and 4 are mutants that **stayed green** and are recorded as non-certifying instead of as passes: `s2`
(join the first `.jsonl` in the state root — at that point the first `.jsonl` was that run's own journal),
`s6` (force the printed join flag — that flag was a derived print, which is why the summary now counts
stamps), `t2` (claim removed — by construction it pinned nothing else) and `t4` (base-text disjunct broken —
the live disjunct in that run is the candidate-body one). The decisive arm for row binding is `v7`: the run's
own three-row journal is copied to an all-zero digest name and the join pointed at it; the log prints
`pool decision rows journal 0000000000000000000000000000000000000000000000000000000000000000 rows 3 stamped 0`
and the run goes red naming `a three-candidate pool writes one ledger row per candidate and stamps each
decision`.

Five audit seats read the increment. The ledger row's `audits` field records each seat's disposition — four ACCEPT WITH NOTES and one ACCEPT on these bytes — and the transcripts are on disk at the measured sizes below; the audit packet quotes each transcript's own verdict line, so this sentence cites the transcripts rather than restating them: `/tmp/mm-audit-pool.out` 28,320 bytes / 635 lines, `/tmp/mm-audit-pool2.out` 17,570 / 323, `/tmp/mm-audit-pool3.out` 19,508 / 405, `/tmp/mm-audit-pool4.out` 30,085 / 667, `/tmp/mm-audit-pool5.out` 2,210 / 30 — 97,693 bytes and 2,060 lines over five logs (`wc -lc` on each). An earlier revision of this section printed a different size set for the same five transcripts; those numbers were not reproducible from the files and are not used anywhere in these four files.; the packets they were given are `/tmp/mm-audit-pool{,2,3,4,5}.json`, 199,284 / 206,690 / 227,280 / 216,785 / 174,231 bytes. Three of their findings are recorded as having been addressed by changing the lane or the product, not only by rewording — seat 1's F2 (an unfailable harness conjunct), seat 2's N1 (a slot-copy claim that was really last-writer-wins coupling) and seat 3's N4 (a journal summary that described a filename shape, not a join); the seat ids and their dispositions are in the ledger row's `audits` field: the room harness and a
candidate worktree copy are different surfaces (the room copy stays put while a slot copy is overwritten by a
refused candidate's body); a slot-copy claim that looked order-independent was last-writer-wins coupling; and
the journal summary's `joined present` described a filename shape rather than a join.
