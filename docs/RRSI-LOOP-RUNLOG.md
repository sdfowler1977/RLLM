# Refinement-loop run log

Transcripts of product runs, not readings of source. Each section is one Gap evidence bar from
`docs/RLLM-recursion-gap-report.md` §4, made on committed bytes inside a `git archive` room.

## Gap 2 — an RRSI run of three rounds on committed bytes — 2026-11-12T19:2Z, commit `b8b0e71`

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

## Gap 3 — task t+1 reading the library entry written by task t — 2026-11-12T19:4Z, commit `b8b0e71`

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
