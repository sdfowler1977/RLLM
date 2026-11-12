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
