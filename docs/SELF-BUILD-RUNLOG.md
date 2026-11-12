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

## Not yet witnessed

The proposal -> approval -> execute leg driven by a model turn (this log drives the ops directly),
and any connected verification group covering self_build — a new group would add a `verification/*.py`
row, which GAP5 is meant to remove rather than grow.
