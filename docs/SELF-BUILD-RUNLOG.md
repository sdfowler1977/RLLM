# self_build run log (GAP1)

Run log for the self-rebuild seam on committed bytes: `edit -> build -> test -> journal commit ->
re-exec -> continuing the same job`. Produced 2026-11-12 against commit `2f44536`
(Add the self_build seam). The room was a clean extract of that commit, so every digest below is
reproducible from the baseline tree.

## Room

```
git archive 2f44536 | tar -x -C .qwen/tmp/gap1-room
ln -s /home/shawn/RLLM/tooling .qwen/tmp/gap1-room/tooling     # pinned toolchain, skipped by the walk
cp output/headless/RLLM .qwen/tmp/gap1-room/output/headless/RLLM
set -a; . tooling/env.sh; set +a                               # CLIENT_RUST and friends
```

`tooling/`, `output/`, `.git/`, `target/`, `.qwen/` are in the walker's `SKIP_DIRS`
(`core/self_build.rs:32`), so they never enter the source digest.

## Leg 1 — fingerprint the committed tree

```
{"op":"self_fingerprint","root":".","binary":"output/headless/RLLM","journal":"self/journal.jsonl","rollback":"self/rollback"}
{"value":{"binary_digest":"3f752f28b0d344922f317a9912fd4362568df579d3f10dc75f7ffc09c62bd939","files":337,
 "fingerprint":"22189804bee3dfc47647dfecff7dcffe46f6cbc6d23a85152f738417896c408e",
 "source_digest":"e4091a0d16fe16bcf7bb6a1d477bdfec26a5579dd6ed87ab9d122259a261d5f7","stale":false,"symlinks_skipped":0}}
{"op":"self_echo","echo":"pre-reload"}
{"error":"Unknown operation"}
```

`self_echo` is not a verb in the committed source. That refusal is the control leg for Leg 6.

## Leg 2 — edit, then let the product build, gate and commit

Edit made in the room (`core/lib.rs`, after the four `self_*` verbs):

```rust
"self_echo" => Ok(obj(vec![("canary", s("successor-build"))])),
```

```
{"op":"self_journal","root":".","binary":"output/headless/RLLM","previous_binary":"output/headless/RLLM.pre",
 "successor_binary":"output/headless/RLLM","journal":"self/journal.jsonl","rollback":"self/rollback","parent":"",
 "sanctioned":true,"units":"output/units-run","review_confirmed":true}
{"value":{"build_digest":"32722da867b47d3e355cd4a4c45154ae2840b04bcd33757a6906df0f38a1777c","builder_exit":0,
 "gate":{"cases":538,"green":true,"isolated":55,"passed":538},"parent":"","recorded":true,
 "revision":"d4fdee927f6bd17017d5a955e12fabd2d450085a69e321b858fc693efec07bdc",
 "rollback":"self/rollback","rollback_digest":"3f752f28b0d344922f317a9912fd4362568df579d3f10dc75f7ffc09c62bd939",
 "schema":"1","source_digest":"e51944eba2674ed21fd805c268b43949dcca228a4348c0e2f99b3bf859b144f9", ...}}
```

One call ran the whole sanctioned plan — `bash headless.sh`, `bash tools/build-units.sh output/units-run`,
`python3 tools/checks.py --all-units output/units-run` — as bounded children with process-group
cancellation, parsed the gate line off stdout (not the exit code), and appended revision
`d4fdee92…`, recording the pre-edit binary digest as the rollback artifact.

## Leg 3 — staleness is detected, not assumed

```
{"op":"self_fingerprint","root":".","binary":"output/headless/RLLM.pre",
 "source_digest":"e4091a0d16fe16bcf7bb6a1d477bdfec26a5579dd6ed87ab9d122259a261d5f7","journal":"self/journal.jsonl","rollback":"self/rollback"}
{"value":{... "claimed_source_digest":"e4091a0d16fe…","source_digest":"e51944eba267…","stale":true}}
```

## Leg 4 — reload refuses without review

```
{"op":"self_handoff","root":".","binary":"output/headless/RLLM","args":["--jsonl"],"review_confirmed":false}
{"value":{"argv":["./output/headless/RLLM","--jsonl"],"binary_digest":"32722da867b4…","executed":false,
 "log":"pid=375399 argv=[…] binary_digest=32722da867b4…","pid":375399,"source_digest":"e51944eba267…"}}
```

## Leg 5 — re-exec, and the same job continues

Driver was the **pre-edit** binary (`output/headless/RLLM.pre`), fed over a live pipe so nothing is
over-buffered before the image is replaced:

```
L2 exec (review confirmed; no reply expected)
L3 successor answers a verb that exists only in the rebuilt source:
{"value":{"canary":"successor-build"}}
L4 successor fingerprints the same tree:
{"value":{"binary_digest":"32722da867b47d3e355cd4a4c45154ae2840b04bcd33757a6906df0f38a1777c","files":337,
 "source_digest":"e51944eba2674ed21fd805c268b43949dcca228a4348c0e2f99b3bf859b144f9","stale":false}}
L5 successor appends revision 2 with parent from revision 1:
{"value":{"build_digest":"32722da867b4…","gate":{"cases":538,"green":true,"isolated":55,"passed":538},
 "parent":"d4fdee927f6bd17017d5a955e12fabd2d450085a69e321b858fc693efec07bdc","recorded":true,
 "revision":"1c8ff4e038634bab40de23af087b24a268efb296a11f08bf2ed0048d9bd2df2f",
 "rollback_digest":"3f752f28b0d344922f317a9912fd4362568df579d3f10dc75f7ffc09c62bd939"}}
```

`self_echo` answering is the discriminator: that verb exists only in the rebuilt source, so the
process that answered is the successor image, still on the same pipe, appending to the same
journal chain (`d4fdee92…` -> `1c8ff4e0…`).

## Defects this log exposed (both recorded as GAP1 open items)

1. **The tool never applies its own edit.** `operation_reference` says self_build "edits one
   workspace source file", `core/application.rs:3629` puts it in the write posture's
   changed-since-review guard, and `prepare()` captures `approved_previous` — but
   `self_build::run()` never writes `content` to `path`; the disk write exists only in the
   `"write" =>` arm at `core/application.rs:3665`. In this log the edit leg is developer-side for
   that reason.
2. **`handoff` refuses on its own rollback artifact.** `handoff` walks the tree with an empty skip
   list (`tree(&root, &[])`), unlike `fingerprint`/`journal` which pass `skip_paths(value)`. With an
   11,983,904-byte rollback binary at a non-skipped path, both handoff legs returned
   `{"error":"file exceeds 4194304 bytes"}` — the walker's 4 MiB per-file cap. The log continues
   after moving `self/journal.jsonl` and `self/rollback` under `output/`, which the walker skips.
   A product layout that keeps the journal inside the workspace cannot reload.

## Re-run on the repaired seam — 2026-11-12T07:4Z, commit `1598752`

The two defects above are closed in the source, so the log was re-made in a fresh room to see whether
the product still refuses. Room: `git archive HEAD | tar -x -C .qwen/tmp/gap1-room2`, `tooling/`
symlinked in, and the room's own controller built inside the room by `bash headless.sh`
(11,983,664 bytes — over the walker's 4 MiB per-file cap, which is what makes the legs below
discriminating).

```
A journal commit through a NON-skipped workspace path (gate line supplied by the driver,
   "Rust cases passed: 541/541; isolated application cases: 55"):
{"op":"self_journal","root":".","binary":"output/headless/RLLM","previous_binary":"output/headless/RLLM",
 "successor_binary":"output/headless/RLLM","journal":"self/journal.jsonl","rollback":"self/rollback",
 "parent":"","gate_output":"Rust cases passed: 541/541; isolated application cases: 55","review_confirmed":true}
{"value":{"build_digest":"69463f2ef2da…","gate":{"cases":541,"green":true,"isolated":55,"passed":541},
 "recorded":true,"revision":"8c61e0523228cb663dcd644c5b3ca56f88e6f60ae21b6e5fd3cb4fb70cc23607",
 "rollback":"self/rollback","rollback_digest":"69463f2ef2da…",
 "successor":{"argv":["./output/headless/RLLM"],"pid":1164782},"verdicts":["gate-green"]}}

B what that left in the workspace: self/rollback = 11,987,760 bytes, self/journal.jsonl

C negative control - handoff with no journal/rollback keys:
{"op":"self_handoff","root":".","binary":"output/headless/RLLM","args":["continue"],"review_confirmed":false}
{"error":"file exceeds 4194304 bytes"}

D the repair - the same handoff with the journal and rollback named:
{"op":"self_handoff",...,"review_confirmed":false,"journal":"self/journal.jsonl","rollback":"self/rollback"}
{"value":{"argv":["./output/headless/RLLM","continue"],"binary_digest":"69463f2ef2da…","executed":false,
 "pid":1163895,"source_digest":"205f3cfe3e35…"}}

E fingerprint with the same keys: source_digest 205f3cfe3e35…, files 339, stale false.
```

**Defect 2 is closed in the product, not just in a unit test**: with an over-cap rollback store and a
journal inside the workspace, the handoff that used to refuse (`file exceeds 4194304 bytes`) now
proceeds as soon as the request names those paths, and the refusal still happens when it does not, so
the pair discriminates. The journal commit itself also succeeds in that layout (revision
`8c61e052…`, gate 541/541 with 55 isolated cases, rollback preserved).

Defect 1 is closed at the seam and witnessed at unit level (`Saved N bytes to <path>.` plus the three
new refusal cases in the gate), but **not yet through the product's own approval path** — see below.

The sanctioned plan was then re-run through the **consolidated** child runner (one `bounded_child`
delegating to `execution::bounded_process`, the seam's private runner deleted) with the correct
parent, in the same room whose journal and rollback live at non-skipped paths:

```
{"op":"self_journal",...,"parent":"8c61e0523228cb663dcd644c5b3ca56f88e6f60ae21b6e5fd3cb4fb70cc23607",
 "sanctioned":true,"units":"output/units-run","review_confirmed":true}
{"value":{"builder_exit":0,"gate":{"cases":541,"green":true,"isolated":55,"passed":541},
 "parent":"8c61e0523228…","recorded":true,
 "revision":"025ca432ba447be74f5237398c3cd13a86e8faf4bcf134edee22861cec6b470f",
 "rollback":"self/rollback","rollback_digest":"69463f2ef2da…",
 "successor":{"argv":["./output/headless/RLLM"],"binary_digest":"69463f2ef2da…","pid":1176140}}}
```

`bash headless.sh`, `bash tools/build-units.sh output/units-run` and
`python3 tools/checks.py --all-units output/units-run` all ran as bounded children, the gate line was
parsed off stdout (not the exit code), the rollback artifact was preserved, and the journal appended a
**second** revision chained to the first — so the runner consolidation did not break the plan legs.

The first re-run attempt produced four refusals that were **my driver's defects, not product reds**,
recorded so the next driver does not repeat them: the journal directory `self/` did not exist
(`No such file or directory (os error 2)`), a `gate_output` line that is not the gate's own shape is
refused (`unit gate did not print case counts`) even though the rollback artifact was still preserved,
`"revision":"1"` is refused (`revision digest must be empty or 64 hexadecimal characters`), and a
handoff pair is uninformative unless the over-cap artifact is already on disk.

## The model-driven leg — 2026-11-12T07:3Z, commit `0f36aa3`

The leg that was previously unwitnessed is now driven by a **model turn through the product's own
review path**, in a fresh room (`git archive HEAD | tar -x -C .qwen/tmp/gap1-room3`, `tooling/`
symlinked, controller built inside the room by `bash headless.sh` in 1m49s — 11,983,664 bytes).

Driver: the room's own controller over `--jsonl`, one profile (`protocol=chat`, `plain=true`,
endpoint `http://ROOM.test/v1/chat/completions`) and one agent whose tools are
`read,write,self_build`. `plain=true` is load-bearing: without it the request is refused with
`HTTP is disabled. Enable unencrypted access explicitly for a trusted LAN.` and nothing is staged.
Each model reply was delivered as `network_result` with `purpose=model`, the `model_expected` ticket
and a base64 `HTTP/1.1 200 OK` wire carrying an OpenAI-style `tool_calls` message — the same stub
mechanism the connected groups use (`verification/proposal_feedback.py`).

```
control 0 - the pre-edit binary refuses the verb that the edit will add:
{"op":"self_echo","echo":"pre-edit"} -> {"error":"Unknown operation"}
room core/lib.rs: 21,755 bytes, sha256 addbba3643053f02...

control 1 - the model proposes a builder the seam does not sanction (builder="sh"):
staged: dialog=approval, pending.tool=self_build, effect=None
approve -> effect {kind:tool, tool:self_build}
tool op -> {"error":"builder is not one of the sanctioned commands"}

main leg - same proposal with builder="bash", builder_args=["headless.sh"], timeout_seconds=600:
staged: dialog=approval, pending.tool=self_build, agent_pending=true,
        args.previous_exists=true, args.approved_previous=21,728 characters
        (= the 21,755-byte file: 14 multibyte characters, so 27 continuation bytes; the
        changed-since-review guard at core/application.rs:3627-3636 compares Rust strings and
        passed, so the stamp is byte-exact)
approve -> effect {kind:tool, tool:self_build}
tool op -> "Saved 21827 bytes to core/lib.rs. /home/shawn/RLLM/.qwen/tmp/gap1-room3/output/headless/RLLM"
           (the tail is headless.sh's own stdout - the sanctioned builder ran as a bounded child)

after: core/lib.rs 21,827 bytes sha256 7c6478b71605a1c0...; the canary arm is on disk;
output/headless/RLLM is 11,987,760 bytes sha256 69463f2ef2da... (byte-identical to the binary the
earlier room produced from the same source, which is a determinism witness, not a stale file)

{"op":"self_echo","echo":"post-edit"}  -> {"value":{"canary":"successor-build"}}   (new binary)
{"op":"self_echo","echo":"post-edit"}  -> {"error":"Unknown operation"}             (RLLM.pre copy)
{"op":"self_fingerprint",...}          -> stale:false, files 339, source_digest f38b606522eec7...
```

So the seam now runs **edit → sanctioned build** with the edit applied by the product itself, on a
proposal the model produced and the owner approved: `inquiry::prepare` stamps
`previous_exists`/`approved_previous` (`core/application.rs:3822`), the write-posture guard runs at
`core/application.rs:3627-3636`, the `self_build` approve branch at `:3100` re-checks the write
rules, dispatch is at `:3640`, and `self_build::run` applies the reviewed bytes with the write
policy before calling `sanctioned_child`. The refusing arm is real: the same proposal with
`builder="sh"` is refused at the place a build would start.

Scope of this leg, stated exactly: it exercised `builder`, not `gate`, and it did not journal or
reload. The gate arm and the journal commit remain witnessed only through the `self_journal` verb
(the sanctioned legs above), because **`self_build::run` has no journal arm at all** — its arms are
edit, `builder`, `gate`, and `reload=exec|spawn`. A model-driven job therefore cannot yet perform
gap-report step 3 (record the revision) inside the tool; it can only do steps 1, 2 and 4.

## Driver defects in this leg (recorded so the next driver does not repeat them)

`shutil.copyfile` does not carry the execute bit, so the `RLLM.pre` control copy raised
`PermissionError` until it was `chmod`-ed; a profile without `plain=true` never reaches the network
stage, so the first send produced no model request and no pending record; and a control whose
expected marker is the *edited* arm cannot discriminate a control that wrote a different marker —
the first control checked for the canary instead of its own `// canary` suffix and reported a
meaningless `False`.

## The single-job chain — 2026-11-12T09:3Z, commit `55e1e96`

This is the log the `GAP1` done-bar asks for: one model-proposed, owner-approved job that edits a
workspace source, rebuilds, runs the unit gate, and records the revision, then a successor built from
those bytes answering the edited behaviour. Room `.qwen/tmp/gap1-chain` = `git archive HEAD` plus the
`tooling` symlink; inside it `bash tools/build-tools.sh` (7 tools, executed-equals-declared counts),
`bash tools/build-units.sh output/units-pre` and `bash headless.sh` (product, 12,052,688 bytes, sha256
`90bed8af12b3…`). Driver `/tmp/gap1-chain.py`, log `/tmp/gap1-chain.log`. Pre-edit control: the copied
`RLLM.pre` answers `{"op":"self_chain_a"}` with `{"error":"Unknown operation"}`.

Job 1 — the model proposed `self_build` with `builder=bash tools/build-units.sh output/units-post`,
`gate=output/tools/gate output/units-post`, `journal=self/journal.jsonl`, `rollback=self/rollback`,
`parent=""`. Product staged it (`dialog=approval`, `pending.tool=self_build`, `agent_pending=true`),
owner approved, dispatch produced:

```
Saved 21834 bytes to core/lib.rs. recorded revision 8766fb5f1e53472c17bbb1d7f7b22a11a6daddffe7a2d01fc42ca8b78430458f in self/journal.jsonl Rust cases passed: 551/551; isolated application cases: 55
```

so edit → build → gate → journal happened inside one reviewed job, with the gate run over the units
binary built *after* the edit. The room's units binary grew from 548 to 551 cases because the room is
`55e1e96`, whose `self_build` carries three more cases than the live tree's 548 baseline.

Job 2 — `builder=bash headless.sh`, `gate=output/tools/gate output/units-post`, `parent=<job 1 revision>`,
`reload=spawn`:

```
Saved 21908 bytes to core/lib.rs. recorded revision 9c55267eac3f2f54ccc86a36c75e19d83a6620aa5fe45a03f016f307cb0d4ab1 in self/journal.jsonl successor pid 1787998
```

Job 3 — `builder=bash tools/build-units.sh output/units-final`, gate on those final bytes,
`parent=<job 2 revision>`:

```
Saved 21908 bytes to core/lib.rs. recorded revision d84af5e809575fa8daf32f679082e519eaae97fb41320c6cb0627543d91cf1ef in self/journal.jsonl Rust cases passed: 551/551; isolated application cases: 55
```

The re-exec leg is discriminating, not asserted. The rebuilt product answers both inserted ops —
`{"op":"self_chain_a"}` → `{"value":{"chain":"gate-units"}}` and `{"op":"self_chain_b"}` →
`{"value":{"chain":"product-rebuild"}}` — while the pre-edit copy still answers `Unknown operation`.
Product binary sha moved `90bed8af12b3…` → `fb1360cc642e…`, matching the `build_digest` the journal
record for job 2 carries.

The journal was then verified independently of the product: all three rows recompute their own revision
(sha256 over the stored body with `revision` removed, canonical compact form), the `parent` links chain
`8766fb5f → 9c55267e → d84af5e8`, and every row carries `verdicts ["gate-green"]` with
`gate {passed 551, cases 551, isolated 55, green true}`. Re-running the room's own gate over the final
units bytes gives `Rust cases passed: 551/551; isolated application cases: 55` rc 0 — the same counters
the journal recorded, measured outside the product's child run.

Scope and limits of this log, stated rather than buried. The gate that closes job 2 is over job 1's
units bytes, not job 2's own edit — a single sanctioned builder per job means job 2 rebuilds the product
while the gate runs the units binary already built post-edit-A; job 3 exists to close that hole by gating
the final bytes. The spawn successor is spawned with no argv and no environment, so "the same job
continues" is witnessed by the journal plus the rebuilt bytes answering the edited ops, not by the
successor process carrying the session. And the two defects this log measures in the arm itself: the
journal's `successor.pid` is `std::process::id()` (the product, `1773485`) while the receipt reports the
real successor `1787998`, and the record carries no `job` or `instruction` field although the tool
description promises the successor inherits them. Driver defects, for the next driver: a tool call whose
`name`/`arguments` are not nested under `function` is refused with `Tool proposal has no completed
provider turn` / `Missing function` and stages nothing, and an `op` helper that returns the reply
envelope instead of its `value` key raises a `KeyError` long after the product actually refused.

## Still unowned

No connected verification group covers `self_build`, and `self_build` is absent from
`REFINEMENT_TOOLS` (`core/harness.rs:274`) while present in `const TOOLS`; adding a group would add a
`verification/*.py` row, which `GAP5` is meant to remove rather than grow.

## The successor is named honestly — 2026-11-12T10:5Z, commit `dbddae2`

The two defects the single-job chain measured in the arm itself are closed in the source and witnessed
through the product. Room `.qwen/tmp/gap1-pid-room` = `git archive HEAD` (at `2bb0267`) + the `tooling`
symlink + the repaired `core/self_build.rs` and nothing else from the live tree, so every number below is
attributable to that one file. Inside it: `bash tools/build-units.sh output/units` then the Python gate ->
`Rust cases passed: 564/564; isolated application cases: 55` rc 0 (560 at `HEAD`, +4 new cases), and the
focused run `output/units self_build` selects exactly 16 tests (12 before), 16 passed / 0 failed / 548
filtered. `bash headless.sh` built the room product (12,180,952 B). Driver `/tmp/gap1-pid-witness.py`, log
`/tmp/gap1-pid-witness2.log`.

One model-proposed, owner-approved job with `reload=spawn`, `job=gap1-pid-witness`,
`instruction="record the successor process honestly"`, `successor_args=["--jsonl"]`, builder
`tools/build-units.sh output/units-w` and gate `output/tools/gate output/units-w`:

```
Saved 21845 bytes to core/lib.rs. recorded revision 450b1bef544761a8b0338a19faedacfba2ae829ce88c42f701f43b362b627938 in self/journal.jsonl successor pid 2175150
```

and the row the journal actually holds, read out of the file by an independent reader
(`/tmp/journal-check.py`, which recomputes the revision digest from the stored body):

```
row 0 {"digest_ok": true, "parent_ok": true, "gate": {"cases": 564, "green": true, "isolated": 55, "passed": 564},
       "argv": ["…/output/headless/RLLM", "--jsonl"], "env": ["RLLM_SELF_JOB", "RLLM_SELF_INSTRUCTION"],
       "instruction": "record the successor process honestly", "job": "gap1-pid-witness",
       "mode": "spawn", "pid": 2175150, "revision": "450b1bef…", "verdicts": ["gate-green"]}
```

The discriminator is `/proc`, not the receipt. While the product was still alive, `/proc/2175150/stat`
reported state `Z`, parent `2171194`, command `(RLLM)` — and `2171194` is the product's own pid, which the
driver knows independently. So the recorded pid is a real second process that the product spawned, not the
product (`same pid as the product? False`), and the receipt and the row cannot disagree because the same value
is used for both. Before this change the row stored the current process's pid while the receipt named a
different one (1773485 vs 1787998 in the chain above).

Also witnessed: the refusal path of the native gate is reachable from the product (`unit gate tool is absent;
build it with bash tools/build-tools.sh` — that is what the first attempt printed in a room where
`tools/build-tools.sh` had not been run), and the job's own limit is stated honestly — it built only the unit
binary, so the control leg confirms the still-un-rebuilt product refuses the inserted op
(`{"error":"Unknown operation"}`), unlike the chain section above where `headless.sh` was the builder.

What this does **not** close. Nothing in `core` reads `RLLM_SELF_JOB` or `RLLM_SELF_INSTRUCTION`: the row and
the child's environment now carry the job identity, and the unit witness proves the child receives the bytes,
but the successor does not act on them, so gap-report step 5 ("continue the same job") is still a carried
identity rather than a resumed job. The spawn arm never waits on the child, which is how `/proc` caught it as
a zombie — the parent does not track or reap its successor. `self_build` is still absent from
`REFINEMENT_TOOLS` and no connected group exercises it.

## One job survives the reload, and the parent reaps its successor — 2026-11-12T13:2Z, commit `3390359`

Room `.qwen/tmp/gap1-chain-room2` is `git archive` of `3390359` plus a symlinked `tooling/`, with
`output/units` (581 cases), `output/tools` (8 native tools) and `output/headless/RLLM` built inside it,
so nothing below can be attributed to any other lane's file. Driver `/tmp/gap1-chain-witness.py`,
log `/tmp/chain-witness2.log`, journal `self/journal.jsonl` in the room. Each leg edits
`core/lib.rs` (appending `pub fn self_chainwitness_*() {}` items, which compile), builds with
`bash tools/build-units.sh output/units` and gates with `output/tools/gate output/units`.

Leg 1 — `reload=spawn`, `job=gap1-chain-job`, `instruction=continue the same job after reload`:

```
Saved 21798 bytes to core/lib.rs. recorded revision abff31fd087c94d8cce06d66e10c065f4f47c082
abbdf9d174542b34a4af060d in self/journal.jsonl successor pid 3061720
```

`/proc/3061720/stat` read while the parent product (pid 3059815) was alive reported state `Z`,
ppid `3059815`, comm `(RLLM)`: the named pid is the product's own child, and a zombie is exactly what
the pre-lane seam left behind.

Leg 2 — same product process, `reload=''`, builder and gate again:

```
Saved 21831 bytes to core/lib.rs. recorded revision 630c16acf19ea578878d394aa79098165a260041
d10c39e55241034d14111dbc in self/journal.jsonl Rust cases passed: 581/581; isolated application cases: 55 reaped 1 successor(s)
```

and `/proc/3061720` no longer exists. Before: a zombie child of the product. After the next
self-build call in the same process: gone. That is the `try_wait` registry doing its job, witnessed by
an external oracle rather than by the lane's own assertion.

The journal, re-read by an independent reader that recomputes each row's digest (canonical compact
sorted JSON with the `revision` key removed):

```
revision 649e49235319 recomputed_ok True parent            mode spawn pid 3057376 job gap1-chain-job job_source caller
revision abff31fd087c recomputed_ok True parent 649e49235319 mode spawn pid 3061720 job gap1-chain-job job_source caller
revision 630c16acf19e recomputed_ok True parent abff31fd087c mode none pid 3059815 job            job_source none
revision 9fe323fa20ef recomputed_ok True parent 630c16acf19e mode none pid 3065097 job            job_source none
```

Four rows, every digest recomputed, every `parent` equal to the previous row's `revision`. The
no-carried-identity control (a third product process, no environment, `job` omitted) records
`job_source none`, so the new key is not simply always "inherited".

### What that run also caught: the successor could not build itself

The successor leg — a fresh product process carrying exactly what the spawn arm hands over
(`RLLM_SELF_JOB=gap1-chain-carried`, `RLLM_SELF_INSTRUCTION=...`), running its own self-build job with
the `job` argument omitted — **refused its own gate**:

```
reload refused: unit gate is not green (579/581)
```

Measured in the same room with the same unit binary and `--test-threads=1`, changing only the two
environment variables: 16 failures without them (the pre-existing failures of a bare run, whose
fixtures need the sanctioned runner's scratch root) versus 18 with them, and the two extra cases are
named:

```
self_build::checks::journal_mode_is_none_without_reload_and_exec_records_exec_only_when_it_writes
self_build::checks::spawn_journal_stores_job_and_instruction_verbatim_and_absent_job_is_empty
```

The seam's builder and gate children inherit the parent environment, so the job identity meant for a
successor product reached the product's own test children. The fix belongs at the boundary: build and
test children must not see the carried identity. Lane C4 closes it, together with a second defect from
the same room — an edit that does not parse produces 7,078,364 bytes of rustc stderr, and the seam
refuses with `builder output exceeded its bound` **before** it ever reports the child's exit status, so
a broken edit hides its own compile error.

Still open after this run, and unchanged by it: the successor's exit status is tracked in-process only
(no journal row for it), and no connected verification group drives `self_build` at all
(`grep -rln self_build verification/*.py` returns nothing).

## The same witness, re-run after the boundary fix — 2026-11-12T14:5Z, commit `63bf5c5`

Lane C4 moved the fix to the spawn boundary: `scrub_carried` (`core/self_build.rs:476`) removes
`RLLM_SELF_JOB` and `RLLM_SELF_INSTRUCTION` with `Command::env_remove`, and `bounded_child` (`:529`) —
which is what actually spawns the builder, gate and probe children — calls it before every child. The two
reload arms still hand the pair over: the exec arm keeps calling `execution::exec_replace`, the spawn arm
scrubs and then re-sets both names on the child. The bound refusal (`bound_refusal`, `:522`) keeps its old
leading clause and now appends the child's exit status and the last 400 bytes, so a broken edit names its
own compile error instead of only its size.

Rebuilt in the same room (`.qwen/tmp/gap1-chain-room2`, tree = `3390359` plus C4's `core/self_build.rs`,
`bash tools/build-units.sh output/units` then `bash headless.sh`) and re-ran the identical witness script.

Run A — one product process, spawn then reap:

```
job 1 (reload=spawn): ... recorded revision bcf8f2846245... successor pid 3141890
/proc before the next call: {'state': 'Z', 'ppid': '3136216', 'comm': '(RLLM)'}
job 2 (reload=none, same process): ... recorded revision 4d7b5c635f77... Rust cases passed: 585/585; isolated application cases: 55 reaped 1 successor(s)
receipt names a reap? True
/proc after the next call: None
product exit: 0
```

Run B — a fresh product process carrying exactly what the spawn arm hands over, `job` omitted. This is the
leg that was red before the fix, and it is now green:

```
job 3 (inherited identity, gate only): Saved 21864 bytes to core/lib.rs. recorded revision 7283200b3f26... Rust cases passed: 585/585; isolated application cases: 55
```

`unit gate is not green (579/581)` is gone. The control leg (no carried environment, `job` omitted) still
records `job_source none`, so `inherited` is a measured branch, not a constant. The journal after the run,
with each row's digest recomputed by an independent reader:

```
revision 649e49235319 recomputed_ok True parent  mode spawn pid 3057376 job gap1-chain-job         job_source caller     instruction_len 34
revision abff31fd087c recomputed_ok True parent 649e49235319 mode spawn pid 3061720 job gap1-chain-job         job_source caller     instruction_len 34
revision 630c16acf19e recomputed_ok True parent abff31fd087c mode none  pid 3059815 job                      job_source none       instruction_len 0
revision 9fe323fa20ef recomputed_ok True parent 630c16acf19e mode none  pid 3065097 job                      job_source none       instruction_len 0
revision bcf8f2846245 recomputed_ok True parent 9fe323fa20ef mode spawn pid 3141890 job gap1-chain-job         job_source caller     instruction_len 34
revision 4d7b5c635f77 recomputed_ok True parent bcf8f2846245 mode none  pid 3136216 job                      job_source none       instruction_len 0
revision 7283200b3f26 recomputed_ok True parent 4d7b5c635f77 mode none  pid 3148160 job gap1-chain-carried   job_source inherited  instruction_len 41
revision 05ca17d80461 recomputed_ok True parent 7283200b3f26 mode none  pid 3149289 job                      job_source none       instruction_len 0
```

Row 7 is the one the gap report asked for: a product process that never saw a `job` argument journals the
job it was *handed*, records `job_source inherited`, and chains onto row 6, the revision that spawned it.

### The same fix removed five thread-sensitive cases

C4 also stopped the unit cases from mutating the process-global environment. Measured by the coordinator
with the two room binaries, default thread count, one variable changed (C4's file), names compared exactly:

| binary | tree | passed | failed |
|---|---|---:|---:|
| `output/units-B5v` | `f2a6ad8` (607 cases) | 582 | 25 |
| `output/units-C4v` | `63bf5c5` (611 cases) | 592 | 19 |

Zero failure names appear only in the second column. Six names appear only in the first: the five
carried-environment cases (`spawn_journal_stores_job_and_instruction_verbatim_and_absent_job_is_empty`,
`tracked_carried_identity_is_recorded_and_handed_to_the_successor`, `tracked_quiet_row_records_job_source_none`,
`tracked_reap_reports_an_exited_successor_once`, `tracked_reaped_text_appears_only_after_a_successor_exits`)
plus `application::scene::checks::queued_close_releases_admission_and_does_not_render`, which C4 does not
touch and which is scheduling-flaky, not something this change fixed. The first attempt at this comparison
printed zero failures for both sides because the grep pattern did not match the runner's row shape; the
numbers above come from re-parsing the saved output and cross-checking it against the `test result:`
summary lines.

Sanctioned-runner truth is unchanged: `python3 tools/checks.py --all-units` always passes
`--test-threads=1`, and under it the tree gates 611/611 + 55.

## The whole chain in one job, on committed bytes — 2026-11-12T16:5Z, commit `57b063f`

This is the GAP1 §4 witness: **edit → build → test → journal commit → re-exec → continuing the same job**,
one live pipe, one workspace, no restart of the driver between legs. Driver `/tmp/g1run4.py`, log
`/tmp/g1run4.log`, room `/tmp/g1run4-room`.

### Room

```
rm -rf /tmp/g1run4-room && mkdir -p /tmp/g1run4-room
git -C /home/shawn/RLLM archive HEAD | tar -x -C /tmp/g1run4-room   # HEAD = 57b063f5fea5a429…
ln -s /home/shawn/RLLM/tooling /tmp/g1run4-room/tooling
mkdir -p output/self output/tools output/headless
bash tools/build-tools.sh output/tools > output/tools-build.log 2>&1; tools_rc=0
bash headless.sh > output/headless-build.log 2>&1;                 headless_rc=0
cp output/headless/RLLM output/headless/RLLM.pre                   # the pre-edit image
```

`output/tools/gate` and `output/headless/RLLM.pre` (12 913 504 bytes) both exist before the first verb, which
is what the first attempt of this run forgot (see the defect list below). The room build prints 46 `^warning:`
lines of pre-existing core dead code; nothing in this run changes that.

### Leg 1 — the committed tree, and the control that the verb does not exist yet

```
{"op":"self_fingerprint","root":".","binary":"output/headless/RLLM.pre",...}
{"value":{"binary_digest":"20f410e2f081b2547f8ae9c0f16cba96db53e37fe2f06837dab96856092beeee","files":346,
 "fingerprint":"63177281b0eefb1abcc16e3ac46244be29452c05716b16c38fc111bff49a190a",
 "source_digest":"adc84f512732b76728e5bbc4dc16fff648c06a535549e3f4ddddc9abaaf82c00","stale":false}}
{"op":"self_echo","echo":"pre-edit"}
{"error":"Unknown operation"}
```

`core/lib.rs` in the room is `git show HEAD:core/lib.rs`, sha256
`7463fcf335b2d88d51574acce4e24cdbec94f90da0311f6084e5c86316f1c8fc`.

### Leg 2 — the edit

One arm added after the `self_handoff` arm of `core/lib.rs`, and nothing else:

```rust
"self_echo" => Ok(obj(vec![("canary", s("successor-build"))])),
```

`core/lib.rs` is now 21 918 bytes, sha256 `1a487c17a12730309b7a445eccb9fbe66c60964c7d21f8ebe2f4f8bd5a5c8b79`.

### Leg 3 — the pre-edit image builds, gates and commits the journal over a live pipe

```
{"op":"self_journal","root":".","binary":"output/headless/RLLM.pre","previous_binary":"output/headless/RLLM.pre",
 "successor_binary":"output/headless/RLLM","journal":"output/self/journal.jsonl","rollback":"output/self/rollback",
 "parent":"","sanctioned":true,"units":"output/units-run","review_confirmed":true}
{"value":{"build_digest":"20f410e2f081b254…","builder_exit":0,
 "gate":{"cases":855,"green":true,"isolated":57,"passed":855},"parent":"","recorded":true,
 "revision":"e623ae04581987e350ac11e9f68ef19c42063825fc90441c8effd1f347b6bcf0",
 "rollback":"output/self/rollback","rollback_digest":"20f410e2f081b254…",
 "source_digest":"cabd5a21b83f0d65ab34e2c9fa5d20b26ade5a3c709ce081e1a6b1198f009bfa",
 "successor":{"argv":["./output/headless/RLLM"],"binary_digest":"fc3cf889e4c43861…","pid":2552296},
 "verdicts":["gate-green"]}} (221.9s)
```

One call ran the sanctioned plan (`bash headless.sh` → `bash tools/build-units.sh output/units-run` →
`output/tools/gate output/units-run`) as bounded children and parsed the gate line, not the exit code: the
post-edit tree gates **855/855 with 57 isolated application cases**. The revision is appended to
`output/self/journal.jsonl`; `output/self/rollback` is the 12 913 504-byte pre-edit image whose digest is
`20f410e2…`, and `build_digest` names the image that performed the build. The successor image written by that
build is `fc3cf889e4c43861…`.

### Leg 4 — staleness is computed, not assumed

```
{"op":"self_fingerprint", ... "source_digest":"adc84f512732b767…"}
{"value":{"claimed_source_digest":"adc84f512732b767…","source_digest":"cabd5a21b83f0d65…","stale":true}}
```

### Leg 5 — reload without review does not replace the image

```
{"op":"self_handoff","root":".","binary":"output/headless/RLLM","args":["--jsonl"],"review_confirmed":false}
{"value":{"argv":["./output/headless/RLLM","--jsonl"],"binary_digest":"fc3cf889e4c43861…","executed":false,
 "pid":2552296,"source_digest":"cabd5a21b83f0d65…","successor_rows":0,"successor_tip":"","successors":[]}}
```

### Leg 6 — re-exec, and the same job continues

```
{"op":"self_handoff","root":".","binary":"output/headless/RLLM","args":["--jsonl"],"review_confirmed":true}
   (the image is replaced in place; no reply is expected from this op)
{"op":"self_echo","echo":"post-reload"}
{"value":{"canary":"successor-build"}} (0.0s)
{"op":"self_fingerprint","root":".","binary":"output/headless/RLLM",...}
{"value":{"binary_digest":"fc3cf889e4c43861…","source_digest":"cabd5a21b83f0d65…","stale":false}} (0.2s)
{"op":"self_journal", ... "parent":"e623ae04581987e3…","sanctioned":true, ...}
revision 2 = e494f78fc9efb849717139d66328c05772f925398993ff9523748b776c1d75d9   (build_digest fc3cf889…)
{"op":"self_echo","echo":"after-revision-2"}
{"value":{"canary":"successor-build"}} (0.0s)
successor exit: 0
```

`self_echo` answers **only** because the Leg 2 edit is live in the process that the same pipe is now talking
to: the control in Leg 1 printed `{"error":"Unknown operation"}` for the identical op. The journal on disk is
two parent-linked rows —

```
{"revision":"e623ae04581987e3…","parent":"","gate":{"cases":855,"green":true,"isolated":57,"passed":855},
 "build_digest":"20f410e2…","rollback_digest":"20f410e2…","verdicts":["gate-green"]}
{"revision":"e494f78fc9efb849…","parent":"e623ae04581987e3…","gate":{"cases":855,"green":true,"isolated":57,"passed":855},
 "build_digest":"fc3cf889…","rollback_digest":"20f410e2…","verdicts":["gate-green"]}
```

— so the successor re-gated the tree it was built from and recorded its own revision under the revision the
pre-edit image wrote. `wc -l output/self/journal.jsonl` = 2.

What this log does **not** claim: the `job`/`instruction` fields are journal fields, not handoff fields
(`self_handoff` accepts only `root, binary, args, source_digest, review_confirmed, parent, revision, journal,
rollback`), so "the same job" here is the same live session, the same recorded chain and the same edited tree,
not a carried `RLLM_SELF_JOB` string — the carried-job route lives in the reviewed-edit path
(`apply_reviewed_edit` → builder → gate → journal → reload with `RLLM_SELF_JOB`), which is not reachable from
these four JSONL verbs.

### Driver defects in this attempt (recorded so the next driver does not repeat them)

| Attempt | What the driver got wrong | What it printed |
|---|---|---|
| `/tmp/g1run2.log` | the exec op carried `job` and `instruction`, which `self_handoff` refuses | `{"error":"unknown self-build field instruction"}`, then every reply shifted by one because the image was never replaced |
| `/tmp/g1run3.log` | pre-created `output/self/rollback` as a **directory** | `self_journal` ran 235.4 s (build + gate) and then refused: `./output/self/rollback must be a regular non-symlink file`. Its exec leg was green: the successor answered `self_echo` with `{"canary":"successor-build"}` |
| attempt 1 of `g1run4` | ran `git show HEAD:core/lib.rs` **inside** the room, which has no `.git` | the redirect truncated `core/lib.rs` to empty (sha256 `e3b0c442…`), and the driver died at Leg 2 |
| attempt 2 of `g1run4` | wrote `> output/tools-build.log` in a room with no `output/` yet | `tools_rc=1`, `headless_rc=1`, `cp: cannot stat 'output/headless/RLLM'` — neither build had actually run |

## The whole chain again, on the bytes this document is committed against — 2026-11-13T12:30Z, commit `1cbc272`

Same seam, re-driven because the earlier whole-chain log was made at `57b063f`. Driver `/tmp/g1run5.py`
(a copy of `/tmp/g1run4.py`, room and log paths renamed), transcript `/tmp/g1run5.log`, room
`/tmp/g1run5-room`. A python driver is still the instrument here; the delivered verification of this seam is
the native `self_rebuild` group, whose own run log on `d31ec6c` is recorded in
`docs/RLLM-gap-status-2026-11-13.md` §Gap 1.

```
git -C /home/shawn/RLLM archive HEAD | tar -x -C /tmp/g1run5-room   # HEAD 1cbc272e5af75559…
bash tools/build-tools.sh output/tools                              # tools_rc=0, warning lines 0
bash headless.sh                                                    # headless_rc=0
output/headless/RLLM.pre 13,096,584 bytes   output/tools/gate 4,731,768 bytes
git -C /home/shawn/RLLM show HEAD:core/lib.rs > core/lib.rs          # sha 7463fcf335b2d88d… (from the repo, not the room)
```

The build prints **zero** `^warning:` lines now; the same room build at `57b063f` printed 46.

| leg | what ran | what the product said |
|---|---|---|
| 1 | `self_fingerprint` on the pre-edit image over the pre-edit tree; then `self_echo` as the control | `binary_digest 832e2933…`, `files 351`, `source_digest 83707f67…`, `stale:false`; control → `{"error":"Unknown operation"}` — the verb does not exist in the image that is about to do the building |
| 2 | one edit to `core/lib.rs` after the `self_handoff` arm (adds a `self_echo` arm) | 21,918 bytes, sha256 `1a487c17a1273030…` |
| 3 | `self_journal` over a live pipe, `binary = previous_binary = RLLM.pre`, `successor_binary = RLLM`, `sanctioned:true`, `units: "output/units-run"` | 209.3 s → `builder_exit:0`, **`gate: {"cases":878,"green":true,"isolated":57,"passed":878}`**, `revision 27b2667eed0987b2…`, `parent ""`, `rollback_digest` = the pre-edit binary digest, `successor {argv:["./output/headless/RLLM"], pid 2529239, binary_digest ff301323…}`, `verdicts:["gate-green"]`, `recorded:true` |
| 4 | `self_fingerprint` claiming the **pre-edit** `source_digest` | `stale:true`, `claimed_source_digest 83707f67…` against the tree's actual `f8a5908d…` — staleness is computed from the tree |
| 5 | `self_handoff` with `review_confirmed:false` | `executed:false`, `successors:[]`, `successor_rows:0` — reload refuses without review, and the successor the builder had already started is not adopted |
| 6 | `self_handoff` with `review_confirmed:true` (exec: the image is replaced, no reply expected), then the **same pipe** | `self_echo` → `{"canary":"successor-build"}`, a string that exists only in the post-edit build, then `self_fingerprint` → `binary_digest ff301323…`, `stale:false` |

`output/self/journal.jsonl` holds the revision chain (`wc -l` 1 at this point: one recorded revision,
`parent ""`), and `output/self/rollback` is the 13,096,584-byte pre-edit image the product saved for itself.

What this re-run adds over the `57b063f` log: the gate row now reports **878 cases with 57 isolated** where the
sections above record `{"cases":538,…,"isolated":55}` and `{"cases":541,…,"isolated":55}`, the room build is
warning-free where that one printed 46 `^warning:` lines, and the whole sequence is on the bytes this document
is committed against. What it still does not claim: the carried-`RLLM_SELF_JOB` route is
not reachable from these four JSONL verbs, so "the same job" is the same live session, the same recorded
chain and the same edited tree — unchanged from the section above.

## The commit leg on the product's own verb — 2026-11-13T15:4Z, a room of `065efd3` plus three working-tree files

Three files are in the working tree and are not in `065efd3`: `core/lib.rs` (`+1` — the
`"self_commit" => self_build::commit(&v)` dispatch arm at `:444`), `core/self_build.rs`
(`+338/−3`, now 5,453 lines — the `commit` verb) and `verification/self_rebuild.rs` (`+1054/−11`,
1,111 → 2,154 lines, `const NAMES: [&str; 9]` at `065efd3:29` becoming `[&str; 12]` at `:42`). The lane
therefore gains **three** checks over the committed nine: the tenth (the review-confirmed re-exec leg),
the eleventh, *"a review permit commits the journalled revision and the commit tree carries the edited
source bytes"*, and the twelfth, *"a review permit refuses a same-revision decoy commit and commits the
tracked source tree above it"*. The three working-tree files hash `f57d94d6284b15d765249ac0e4713ec59e8d62327f60e23800699a01868d8ae1`
(`core/lib.rs`), `b3d08984df613689ab93c5785f503587cd2fb953b512540bee6c1bede07a38c5` (`core/self_build.rs`)
and `ceb4748742f7aab9e1ac36faccee39971730a2d5fb1cf073eb13238d5608c24c` (`verification/self_rebuild.rs`);
the per-file line counts come from `git diff --numstat HEAD -- core/lib.rs core/self_build.rs
verification/self_rebuild.rs` = `1 0`, `338 3`, `1054 11`.

### The lane, witnessed in a sanctioned room

`/tmp/g31room.sh`: `git archive HEAD` of `065efd31121593ab51bd89a151dd5f7df4ae72c2` (recorded in
`/tmp/g31room-out/commit.txt`), those three files copied over it (digests in
`/tmp/g31room-out/overlay-shas.txt`), `tooling` symlinked, nothing else. In that room:
`tools_rc=0 gates=18`, `headless_rc=0 warning_lines=0`, `twin_build_rc=0 twin_warnings=0`, `lane_rc=0`,
report `count` 11, `failed` [], `passed` 11, transcript printing **`11 self-rebuild checks passed`**
(`/tmp/g31room-out/self_rebuild.log`, 5,587 bytes). That room ran the lane bytes **before** the twelfth
check existed — its overlay digests are `f57d94d6284b…`, `853bec52246f…`, `49f6bf130afcbd3a…`
(`/tmp/g31room-out/overlay-shas.txt`) — so it is a historical witness of the 11-check bytes; the room that
witnesses the bytes committed here is `/tmp/g42room`, below, whose group run reports 12.

The eleventh check seeds a room repository with one commit, edits `src/edited.rs` to carry a marker,
journals the revision, then calls `self_commit` twice — once with `review_confirmed:false`, once with the
permit. Its evidence line is printed whole, and the conjuncts it enforces are: the unpermitted call
returns no commit (`unpermitted_commit=absent`) and leaves every ref byte-equal (`refs_equal=true`,
`ref_before` = `ref_after_unpermitted` = `930176cda6a2…`) while journaling a commit-intent record
(`intent=true`); the permitted call returns a 40-hex commit whose parent is the pre-commit head, moves
the room ref and `rev-parse HEAD` to exactly that commit, and reports exactly one committed path
(`committed_paths=1 src/edited.rs`); the commit tree carries a `src` entry whose blob is what
`git rev-parse <commit>:src/edited.rs` resolves to (`linked_blob` = `blob` = `608553fb43f5…`), that blob
equals the edited source byte-for-byte and contains the marker (`marker_in_blob=true`,
`blob_equals_edit=true`) while the parent blob does not (`marker_in_parent_blob=false`); and a second
permitted call is idempotent (`second_commit` = `commit`, `second_same=true`, refs stable).

### That check is falsifiable

Battery `/tmp/srbatt.py`, base `49f6bf130afcbd3a1eb2bd797e6f8d1d5154d3ba018c1ed9dbe62afc66f47b9a` (the
11-check lane bytes as they stood before the binding below — that base is *not* the sha256 of the file
committed here, which is `ceb4748742f7…`): **ten arms, ten `-- landing … sha=` lines, zero
SKIP**, the zero-change control `w0` printing `11 self-rebuild checks passed` at rc 0, and nine mutants
red with the failure naming check 11 (`names-check11=yes` × 9). `w1` inverts `marker_in_blob`, `w2`
inverts `blob_bytes == edited`, `w3` inverts `!marker_in_parent`, `w4` inverts the no-ref-movement
conjunct, `w5` inverts `room_head == commit`, `w6` compares the committed path against `"src"`, `w7`
inverts the idempotence conjunct, `w8` requires the parent to be the new commit, `w9` inverts the intent
conjunct. Every arm compiled (`compile_rc=0 errs=0`) and the lane came back byte-identical
(`RESTORE-SHA … unchanged=yes`).

### The audit seat rejected the verb, so the idempotence rule was tightened

Seat G1 (`/tmp/mm-grokdiff.out`, 7,720 bytes, read in full; packet `/tmp/mm-grokdiff.json`, 66,182
bytes, sha `05a13208ebe4d8ad2ab4ee27589f8ada4b9590220aef61c6a15f36434b6d90e8`) confirmed the three limits
already recorded as owed and found more.
Three of its findings were defects rather than limits, all in one place: the idempotent path recognised
an existing commit by its **subject** plus a clean tracked tree, so (a) a same-subject commit with a
different tree was accepted, (b) the journal's `source_digest` was never bound to the commit's own tree,
and (c) a *failed* `git commit` followed by that same subject test was returned as success.
`matching_commit` now also requires the candidate commit's own tree to carry exactly the source path set
this room would commit (the `commit_tree_paths(root, git_dir, &head, skip)? != paths` conjunct;
`write_source_commit` threads `skip` into both its `matching_commit` calls, so the post-failure fallback
is held to the same rule).

What that witness falsifies is one conjunct, and only that one: the arm `p1` turns `!=` into `==` in the
`commit_tree_paths(…) != paths` test (`core/self_build.rs:1232`), so it proves the **tree path set**
binding is load-bearing. Of seat G1's three defects the conjunct closes the first — a same-subject commit
carrying a different path set is no longer recognised — and reduces rather than eliminates the other two:
a failed `git commit` is still returned as success when the post-failure `matching_commit` (`:1302`) finds
a commit whose subject *and* path set match, and the journal's `source_digest` is still never bound to
committed blob contents. No arm in that battery reaches the tree's *contents*, the staging set, or the
journal record, so those stay limits, listed below.

That conjunct is witnessed, not asserted — `/tmp/g36room.sh` re-witnessed the room on the fixed bytes and
then mutated the **product**:

```
tools_rc=0 cases_lines=10 err_lines=0   headless_rc=0 warning_lines=0
twin_build_rc=0 twin_warnings=0         lane_rc=0  (count 11, failed [], passed 11, scope 359)
BASE core/self_build.rs sha b3d08984df613689…
p0 control  rebuild_rc=0  lane_rc=0  11 passed  second_same=true
p1 != → ==  sha 30faecc32fd29cc0  rebuild_rc=0  lane_rc=1
            failed = ["a review permit commits the journalled revision and the commit tree carries the edited source bytes"]
            second_same=false  second_error=git…
restore     RESTORE-SHA b3d08984df613689… unchanged=yes
p2 rebuild_rc=0  lane_rc=0  11 passed  second_same=true
```

Removing the binding does not merely change a sentence: it turns the delivered check red and makes the
second permitted call return a different commit (`second_same=false`).

### The twelfth check, and the guard it exists to exercise

The twelfth check refuses a **same-revision decoy**. Its fixture (`decoy_commit` / `drive_decoy_commit` in
`verification/self_rebuild.rs`) builds a room repository, `git add -f`s `notes/decoy.txt` and then deletes
it from the worktree, and does the same for `output/decoy.bin` staged under `-f`, so the tracked path set
is strictly larger than the head tree set while `git diff --quiet HEAD` still reports a clean tree; it
then makes a decoy commit whose subject is exactly `self-commit {revision}` — the subject `matching_commit`
searches for — and requires the permitted `self_commit` call to produce a commit that is **not** the decoy,
whose `parent_commit` **is** the decoy, that becomes `head_after`, and whose committed blob bytes equal the
edited source. The fixture is built so that the decoy satisfies the two tests `matching_commit` applies
before the tree-path conjunct — the exact subject `self-commit {revision}` and a clean `git diff --quiet
HEAD` — which is what makes the conjunct the discriminator. "Refuses" here is operational, not an error
return: the verb declines to reuse the decoy as its idempotent answer and creates a successor commit whose
parent is the decoy.

That conjunct is measured load-bearing on these bytes in `/tmp/g42room.sh` — `git archive` of
`065efd31121593ab51bd89a151dd5f7df4ae72c2` (`/tmp/g42room-out/commit.txt`), the three working-tree files
copied over it (`/tmp/g42room-out/overlay-shas-before.txt` = `7463fcf335b2…` / `2f58da009458…` /
`2a503b53069d…`, `/tmp/g42room-out/overlay-shas.txt` = `f57d94d6284b…` / `b3d08984df61…` /
`ceb4748742f7…`), `tooling` symlinked, the tools built in the room so the group has `output/tools/gate`
(`gates=18`), transcript `/tmp/g42room.out`:

```
tools_rc=0 gates=18    headless_rc=0 warning_lines=0    twin_build_rc=0 twin_warnings=0
baseline_lane_rc=0     12 self-rebuild checks passed    "count": 12, "failed": [], "passed": 12
                                     (/tmp/g42room-out/self_rebuild-baseline.log:18,34-37)
guard anchor hits: 1   (`core/self_build.rs:1232`, the only occurrence)
GUARD-mutant sha 0c83ad4d8262   (`if commit_tree_paths(…)? != paths` → `if false {`)
PRODUCT-REBUILD[mut] rc=0 warning_lines=3
GROUP[mut] rc=1        passed_line=[]   panicked=[]
   fail_lines=['FAIL a review permit refuses a same-revision decoy commit and commits the tracked
                source tree above it']        "count": 12   "failed": [ that one name ]
INERT sha 8d0b4480f14e   (a comment line inserted above the same guard)
GROUP[inert] rc=0        12 self-rebuild checks passed   "failed": []
RESTORE-SHA b3d08984df613689… unchanged=True   PRODUCT-REBUILD[restored] rc=0 warning_lines=0
REPO-EQUALS-ROOM True  (repo and room file both `ceb4748742f7…`)
```

(`/tmp/g42room.out:1-2,4,6,13-20,24,27-28,34-36,38-46`;
`/tmp/g42room-out/self_rebuild-mut.log:17,34-37`;
`/tmp/g42room-out/self_rebuild-inert.log:18,34-37`.) The mutant arm is the one the earlier battery could
not run: it neuters the guard in an otherwise byte-identical file and exactly one check goes red, the
failure naming the twelfth check, while the inert control — same guard, one comment line added above it —
stays green at 12. So on **these bytes and this fixture**, disabling that guard makes the twelfth check
fail. The `p1` arm above (`!=` → `==`, which reds the eleventh check) is a second mutation of the same
guard on the earlier 11-check bytes, not an independent witness of a second mechanism; the isolation claim
rests on the g42 pair, and it is bounded to this implementation and fixture — it does not show the guard is
the only possible implementation, that the fixture covers every index/worktree/HEAD state, or that the
binding reaches blob contents (the last stays a limitation below).

That pair also answers the axis on which the earlier lane seat rejected the verb: seat L1
(`/tmp/mm-lane1.out`, packet `/tmp/mm-lane1.json`) graded all five mechanism questions SUPPORTED with the
guard and fixture lines quoted, yet its verdict was REJECT because "the twelfth check does not prove that
the tree-path guard, rather than another mechanism, caused the decoy refusal". With only the guard removed
and the file otherwise identical, that one check goes red; with an inert edit at the same place it does
not. That is the guard-level isolation the seat asked for, for this check and this fixture; it is not a
claim that no other mechanism could produce the same verdict.


### Whole-tree gates: the 11-check increment broke nothing whole-tree, and one group was already dead

Because a new product verb can break any group, the three files were gated over the whole tree in a
second room (`/tmp/g32room.sh`, same archive, same three overlays): `headless_rc=0 warning_lines=0`,
`verify_rc=0` (73 binaries under `output/verification` = 72 twins, one per `verification/*.rs`, plus
`twin-helper-tests`; `twin helper tests … cases: 44`), `tools_rc=0` with ten `cases:` lines and no error
line, `harness_build_rc=0`, and `output/harness sweep` → `sweep_rc=1`,
`summary: 64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4156 checks`. That sweep ran the
**11-check** lane bytes, so "broke nothing" below is a statement about those bytes. The 12-check bytes were
not swept whole-tree: `/tmp/g42room` builds the tools, the
product and that one twin and runs the `self_rebuild` group, so the whole-tree statement above is scoped to
the 11-check bytes, and for the bytes committed here the claim is the group result quoted below (12 green)
plus the two build verbs (`tools_rc=0 gates=18`, `headless_rc=0 warning_lines=0`).

The one failure is **not** this increment, measured rather than assumed: `/tmp/g39room.sh` is a byte-copy
room of the same commit with **nothing** overlaid (`room_has_git=no`; `/tmp/g39-verdicts.txt` holds its 71 per-group rows), and its sweep prints
``64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4154 checks` (`/tmp/g39room/sweep.log:76`)`. Both rooms' sweep logs were then parsed to per-group rows (`name verdict check-count`) and
joined by group name (`/tmp/join-g32-g39-now.txt`): 71 rows in each room, and **exactly one row differs** —
`self_rebuild` at `('PASS','11')` in the lane room against `('PASS','9')` in the byte-copy room
(`self_rebuild PASS (exit 0, 2s, 11 checks)` against ``self_rebuild PASS (exit 0, 1s, 9 checks)` (`/tmp/g39-verdicts.txt:55`)`) — which is also the whole of the
4156/4154 check-count difference. A raw `diff` of the two verdict text files is not
this join: `/tmp/g32-verdicts.txt` and `/tmp/g39-verdicts.txt` render the same verdicts in different
columns, so the comparison is on normalized rows, not bytes. `history_migration` fails in both: `tools/harness.rs`
passes `--previous-controller` to that group unconditionally (`:1078` in `065efd3`, `:1086` in the
working tree) while no build verb produces `output/previous-controller`
(`grep -rn previous-controller tools/*.sh headless.sh package.sh` = 0 hits; the only tracked writers of
that path are the two test helpers inside `#[cfg(test)]` in `tools/harness.rs`) and the twin names the
requirement itself (`verification/history_migration.rs:73`), so it panics spawning a path that does not
exist (`verification/twin/mod.rs:249`); the delivered `verification/RESULTS.json` already carries 0
checks for that group and lists it as unavailable, and that entry was republished at `0eb0cb1`
(2026-11-12 19:38:45 -0500, *"Republish the connected record: 71 groups, 4248 checks, 67 pass, 0 fail"*),
which is an ancestor of `54c1f1c` (2026-11-13 10:22:10 -0500) — the commit that landed the native twin now
failing on it. That is an owed fix — supply the reference build or report the group
*unavailable* — recorded in the ledger row rather than argued away. Neither room rewrote the delivered
record, and for the reason the code states rather than one I invented: `commit_sweep` returns before
`publish_sweep` when any row failed (`065efd3:tools/harness.rs:1616`), so both rooms printed
`delivered record left untouched: a group failed`; the byte-copy room's record is unchanged at
``1450f95863f9cc05b3c61e95ce7941d7f3154855`, the `065efd3` blob id of `verification/RESULTS.json`; `/tmp/g39room.out:38` prints `results_before` = `results_after` = `results_committed` = that value`. The same room shape with that row turned unavailable prints
`delivered record written: 71 groups, 4156 checks` (`/tmp/g36room.sh`, `/tmp/g36sweep.log`), which is why
the fix belongs to this lane.

### What this does not claim

The commit leg is witnessed; the re-exec-and-carry-on leg is not, so the chain stays a python-driver
witness (§"The whole chain again, on the bytes this document is committed against", commit `1cbc272`)
until the continuation leg lands. That absence is a census, not an assumption: `grep -rn carry_on
--include='*.rs' core` = 0 hits, the `self_*` dispatch surface is `core/lib.rs:440-444` (five verbs,
`self_fingerprint`, `self_plan`, `self_journal`, `self_handoff`, `self_commit`), none of which continues a
round, and `core/self_build.rs` has no git-push verb — its only `fn push` is the output-buffer accumulator
at `:518`, while `grep -rn '"push"' --include='*.rs' core` returns 3 hits, all
`core/page_program.rs:197`, `:228`, `:247`, which are JS array-method dispatch. Seat G1's
remaining findings are recorded as owed, not closed: the commit's parent comes from the repository's
current HEAD while the request's `parent` is compared only against the journal record; between the
source-digest check and `git add`/`git commit` a concurrent edit can move the tree (no lock, no index
snapshot); the journalled commit-intent record is written on refusal but is not a prerequisite for the
permitted call; `tracked_source_paths` is bounded at 1,024 paths with no bound on staged bytes; and ref
handling covers symlinked `.git`/HEAD, malformed symbolic refs and non-regular ref files, but not
packed-refs or every traversal a repository layout can express. A permitted commit stages **every** path `tracked_source_paths` returns —
`core/self_build.rs:1292` is `add.extend(paths)`, and that set comes from `git ls-files -z` filtered by
`source_rel` and capped at 1,024 paths (`:1242-1257`, refusal sentence `tracked source tree exceeds 1024
paths`) — so the verb commits the whole eligible tracked source set, not only the file the job edited
(check 11 reported `committed_paths=1` only because that room's eligible set was one file). And the
journal's `source_digest` is recomputed from the **worktree** (`tree(&root, &skip)` at `:1390`, compared at
`:1391`), while the accepted commit is bound only to that tree's **path names** (`commit_tree_paths` runs
`git ls-tree -r -z --name-only`, `:1258-1280`) plus a clean `git diff --quiet HEAD` (`:1235`) — never to the
committed blob contents, so a commit carrying the same paths with different bytes satisfies the conjunct.
Nothing here claims a pushed or published commit.
