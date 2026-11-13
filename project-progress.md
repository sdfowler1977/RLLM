# RLLM checkpoint — 2026-11-10__00-23-57Z

Build 1794270237; signed candidate. Auto chooses a direct reply, a short current-fact lookup, or deeper investigation. The conversation has explicit mode choices. Simple local time and date questions use the device clock. Quick lookups keep full source and error details available on tap, and a verified DJIA daily close is labeled with its observation date. Automatic web access now includes ordinary public HTTPS pages and marketplace searches opened directly by primary or delegated agents. The app checks the resolved address before connecting and applies the same public scope to every redirect. Requests for private destinations, page scripts and captures follow existing review controls. Supported OpenAI, xAI, Anthropic and Gemini connections can use provider-hosted search on both the primary agent and delegated agents. Provider searches run on the selected connection, and RLLM keeps returned source metadata. One evidence-only follow-up can recover an empty answer after search without repeating the query. Automatic web search approval is on by default for configured search sources. A single valid search proposal in model prose runs under the same approval policy, and eligible ambiguous proposals get one bounded model correction. Web-search fallback and subsequent queries stay in the current turn. Chat and local streaming enforce declared lengths and encodings and validate chunk boundaries and trailers before adopting text or completion. Bodyless HTTP 204 responses cannot publish reply text. Owned responses enforce the shared 16 KiB trailer limit. Framed MCP control acknowledgments settle after informational headers without waiting for a close. An explicit search deferral requires fresh approval even when automatic or saved approval is enabled, and omits the provider-hosted search tool for that turn. RLLM offers a second bounded refinement round after a comparable partial gain, keeps the stronger tested instructions, and compares JSON values in separate evaluation. Shared response framing accepts split delimiters at the exact 8 KiB chunk-size and 64 KiB response-header limits. RLLM can evaluate the original and retained agent instructions on up to two separate tasks with local phrase, exact-text, JSON or digest checks. Both retained replies must pass comparable checks before the evaluation can contribute a measured lesson. Evaluation proposals, interrupted responses and incomparable contexts cannot promote a lesson. A completed training candidate remains reviewable after an interrupted evaluation. RLLM learns the default built-in search order from validated same-query fallbacks. Checked agent refinements replay the exact revised instructions and retain a passing candidate when a revision fails. User adoption preserves the original agent profile. Duplicate correlated MCP POST stream replies are refused before result adoption. RLLM retains multiple named MCP connections and uses one selected server for discovery, calls and streams. A refresh publishes only the complete tool catalog and refuses duplicate tool names across pages. Ambiguous names reported by the active connection require a successful refresh before use, including after switching away and back. New conversation leads the plus menu. An explicit request to search the web authorizes built-in queries for the current turn without repeated prompts, including a follow-up command that relies on the current topic. An unavailable built-in source can fail over, and raw search diagnostics stay in message details. Higher-priority destination review remains available. Conversation displays compact web results and keeps raw tool JSON in message details. Storage authorization uses the configured certificate hostname on the matching HTTPS route while separate origins retain their own identity. Shared Rust HTTP request and response handling validates field syntax, credential-header boundaries, gateway request lengths and expectations, informational responses and chunk extensions. Optional pre-filled device verification links retain the manual address and code and fall back on unusable links. Safe WebDAV updates also preserve valid UTF-8 opaque ETags in exact conditional writes while retaining weak, malformed and duplicate validator refusals. Shared token adoption refuses non-text refresh credentials before replacing saved grants and preserves scoped uncertain-refresh recovery. Existing conversation, intrinsic investigation, local storage and optional provider-neutral services remain integrated. The signed candidate matches the recorded cumulative source inputs. Recursive refinement can check individual JSON fields in training and held-out tasks. It keeps expected values local and only treats a partial score increase as progress when earlier passing checks still pass. WebDAV listings respect declared XML namespaces, preserving valid file and folder routing when extension elements use similar names. An owned media feed failure retires its ticket before it can mint a completion receipt, including when the HTTP framer has reached its last byte. Structured refinement can unwrap a single JSON code block for local verification and records the repair separately. A failed owned import cannot issue a receipt for its staged prefix. A failed upload read retires only its own ticket before another read can advance it. Media chunks reject malformed Base64 instead of accepting a decodable prefix.

Validation: 64/65 connected groups, 4228 executed checks, 513 Rust cases from the full suite. APK 6932498 bytes, SHA256 86b7d74156a11dc6ee6d70866bb480e30b75bac57c3400878c86b9f205921c3d. Retained signer ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5. The matching signed APK is supplied separately.

Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and whole-product independent acceptance remain unverified. Physical background/screen-off and battery behavior remain unverified. Bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device. The historical revision-two comparator is unavailable; its three compatibility checks remain open. Scene preview displays static triangle geometry with base and vertex colors. Embedded PNG base-color textures and MASK cutouts are displayed. External images and other image codecs retain geometry-only viewing; animation and physical material effects remain open. Skins, morph targets, blended transparency and required unsupported extensions refuse only preview; export remains available. GPU rendering and wider scene formats remain open.

Protected baseline: version1793750802, immutable source SHA2562676d5476035cc32db9b85f652c94ecc5e21ca09f925ac2dc983cce932e94917, baseline commit36a5075caa58110ba33ef58990d91c1a102ae970. Immutable baseline source Drive1pBK9HP49rMqBuftLOhH_IMfgWIs0gIdB, APK1mFRkR3ubuMLrVE0Y0iDroRlWivpChtqi. Do not overwrite those baseline copies.

Next: publish and verify this signed candidate. Broaden product-owned semantic repair and reusable deterministic verification. Continue the protocol, storage and resource roadmap with remaining integration gaps. Recover genuine revision-two source through the selected stable source's revision history if browser authentication becomes available; run all three historical checks without changing the reader fingerprint or current decoder. Complete current-device/live-endpoint and independent acceptance separately. Earlier broader accepted roadmap remains in the canonical cumulative engineering record, not cancelled by this upgrade: https://docs.google.com/document/d/1jyJ1ma1juoWacfJFpei5MdzPS05i7EiNy-FyWs9UPRI/edit . This build does not claim whole-project closure. The existing hourly build task Automation_787bc0676d388191bc52212c823ab48b and hourly delivery task Automation_1188b1045d7481919a5069a82a9def98 were updated and read back. Both are enabled. Their prompts prefer the newest verified project Drive source and reconcile later working changes. The scheduler did not expose the next-run timestamps at this checkpoint. Signing custody remains in the separate protected archive.

# RLLM working checkpoint — 2026-11-12T02:35Z (uncommitted lanes in flight)

Permission modes are complete: harness-wide `review` / `limited` / `yolo`, one mode per saved
connection plus per-connection web and file limits under App settings → Permissions, the mode
selector on the chat page footer (LLM · agent · permission mode · tok/s), and web search standing
down for review in every mode — `permission_modes` (27 connected checks) is wired into the runner,
`checks.py` and `package.py` inventories. A capability with no saved default now resolves only to
the selected connection's own provider host, so a defaulted-away capability still reports as not
ready.

Verification state at this checkpoint: unit gate **532/532** Rust cases; connected coordinator
sweep **69 groups → 65 passed, 0 failed, 4 unavailable**, **4,185** executed checks,
`verification/RESULTS.json` sha256 `7250328b5385`. The four unavailable groups need the signed
companion APK and the three historical reference controllers, all owner-supplied.

The repository substrate now exists: branch `main`, baseline `727a33d` (339 files),
`tooling/` (5.6 GB pinned toolchain), `.qwen/tmp/` and `.playwright-mcp/` ignored. History starts
here, so revisions 1793395702 and 1793592809 are still unrecoverable.

`docs/RLLM-recursion-gap-report.md` is adopted as five open obligations `GAP1`–`GAP5` in
`closure_ledger.json` and in `docs/AUDIT.md`. Gap 1 (self rebuild → revision → re-exec) and Gap 5
(native tooling, removing Python) are in flight with Grok Build: `core/self_build.rs` and
`tools/build-tools.sh` + the `alignment`/`readback`/`checkpoint` port. Measured Python exposure
today: **79 files / 8,004 lines** (`tools/` 444, `verification/` 7,560) — wider than the report's
eight `tools/` files, and it grew this window because the new verification group is Python. That
is an open R-rule violation, not a neutral fact; the standing rule now is that no lane adds a `.py`
without stating why.

Not claimed: device acceptance, the signed APK bytes, PUBLICATION-MANIFEST republish (sources moved
past it), and any Gap 2/3/4 capability.

# RLLM working checkpoint — 2026-11-12T03:05Z

`GAP1` step 1 is in the tree (`2f44536`) and its loop has been **run for real**:
`docs/SELF-BUILD-RUNLOG.md` shows the product fingerprint a committed tree, run the
sanctioned plan (`headless.sh` + `build-units.sh` + `checks.py`) as bounded children, append a
content-addressed journal revision with the pre-edit binary as the rollback digest, detect a stale
claimed digest, refuse a reload without review, then `exec` into the rebuilt image — whose
successor answered a verb that exists only in the rebuilt source and appended the next journal
revision on the same pipe. Two defects came out of that log and are open: the tool never applies
its own edit, and `handoff` ignores the journal/rollback skip paths so its own rollback binary
trips the walker's 4 MiB cap. `self_build` is offered to new agent profiles (28/28 tools, measured
on the built product) but is absent from `REFINEMENT_TOOLS` and no connected group exercises it.

`GAP5` step 1 landed: `tools/alignment.py`, `readback.py`, `checkpoint.py` replaced by Rust plus
`tools/build-tools.sh`, with `package.sh` and `verification/apk_alignment.py` rewired. Four defects
were measured and dispatched (stale `tools/alignment.py` literals in `tools/apk.py:10` and
`tools/package.py:105`; no build step behind `package.sh:101`; 8 unit tests no instrument ran;
`checkpoint.rs` has no consumer at all). Tracked Python is now **76 files / 7,932 lines** from
**79 / 8,004** — and `GAP1` added a **new** Python dependency inside the product
(`core/self_build.rs:435,556,983` name the Python gate), which is stated rather than absorbed.

Also re-measured: `PUBLICATION-MANIFEST.json` pins 162 build inputs — 131 match, 30 drifted, and
**1 names a file that no longer exists** (`tools/alignment.py`); 6 build-input-scoped files are not
in the manifest. `docs/BUILD.md` and `docs/AUDIT.md` no longer describe deleted Python tools.

Not claimed: any `GAP2`/`GAP3`/`GAP4` capability, device acceptance, the signed APK bytes, or a
record republish (no connected sweep has been run since these edits; the record still reads
69 groups / 4,185 checks / sha256 `7250328b5385`).

# RLLM working checkpoint — 2026-11-12T07:0Z

**`GAP5` step 1 landed and was verified by coordinator runs, not by the lane's report**
(`812b23b`, `d57ac13`, `b48b405`):

- `bash tools/build-tools.sh` executed **2 / 1 / 5** cases for `alignment` / `readback` /
  `checkpoint`, each equal to that source's declared `#[test]` count; the builder exits 1 on a
  failed run, an unreadable count, or a mismatch, so those tests cannot silently stop running.
- A connected instrument now drives the port: `apk_alignment PASS (exit 0, 6-7s, 11 checks)` in
  only-mode while `verification/apk_alignment.py` shells `output/tools/alignment` for both align
  and verify, with the truncated / changed / missing / extra / duplicate / CRC-corruption arms
  still refusing. Subjects: `output/2026-11-11__14-59-03Z/package.apk` and
  `output/2026-11-12__05-31-35Z/package.apk`, both 6,944,777 bytes and unsigned.
- `output/2026-11-12__05-31-35Z` is a **real `package.sh` run** (unsigned) made after the fixes: its
  167-entry `build-inputs.sha256` names `tools/alignment.rs` (`f05de95d…`), and that inventory still
  equals the current expected set, so `tools/apk.py:10`'s `'Build input inventory changed after
  compilation'` assert passes on real bytes. Two per-file digests now mismatch because the `GAP1`
  lane edited `core/` after that leg — the drift check working as designed.
- Exposure found in my own change and fixed: the aligner went from an imported source file to a
  **built artifact**, so `tools/harness.rs` now classifies a missing `output/tools/<tool>` as an
  unavailable development input. Measured both arms with a private coordinator build: present →
  `PASS (11 checks)`; moved aside → `UNAVAILABLE (not executed, missing: path output/tools/alignment
  (built by tools/build-tools.sh))`.

**`GAP5` step 2 dispatched to Grok Build 4.7** (background shell `bg_88dc7644`):
`tools/signature.py` + `tools/apk.py` + `tools/toolchain.py` (177 lines) → native programs,
`tools/build-tools.sh` extended, `tools/package.py:4` rewired to the native signature program, the
three `.py` deleted, **no new `.py` anywhere**, and scoped away from `core/` so it cannot collide
with the `GAP1` lane.

**`GAP1` lane still in flight** (uncommitted: `core/` +217/−98). Its observed diff direction is the
briefed one: `apply_reviewed_edit(root, args)` called from `run` (defect 1 — the reviewed edit never
reached disk), `tree(&root, &skip_paths(value)?)` replacing the empty skip list (defect 2 — a
rollback binary above the 4 MiB walker cap made reload refuse), `bounded_child` delegating to
`execution::bounded_process` (removing the duplicated child runner), plus three tests named
`reviewed_edit_lands_on_disk_and_the_receipt_names_the_write`,
`reviewed_edit_refuses_a_symlink_a_directory_and_a_non_utf8_source` and
`handoff_skips_the_journal_and_the_over_cap_rollback_store`. `python3` still appears three times in
`core/self_build.rs`, so `GAP1` still entrenches the Python gate until `GAP5` step 3 makes that gate
step native.

Python exposure measured again: **76 files / 7,932 lines** tracked-and-present (`tools/` 5,
`verification/` 71).

Not claimed: signed-path behaviour (no signed APK exists anywhere under `output/` or `release/` on
this host), `readback.rs`/`checkpoint.rs` behavioural witnesses, device acceptance, any
`GAP2`/`GAP3`/`GAP4` capability, or a record republish — the record still reads 69 groups /
4,185 checks / sha256 `7250328b5385`, because no full sweep has been run while two lanes write.

---

## 2026-11-13 — the delivered record was 117 commits stale, and verification starts going native

Commits this window: `a07fa22` (hosted_phases scoring repair) → `52f7dfe` (stored-capture reader repair)
→ `0eb0cb1` (republished record + ledger) → `6962fda` (`history` becomes a Rust twin) → `c7c670b`
(a twin's report is byte-identical to the leg it replaces).

**The record was not reproducible.** At `4309c84` the sweep printed `63 pass, 4 fail, 4 unavailable,
4 absent of 71 groups, 4028 checks` and `commit_sweep` refuses the whole record when any row fails, so
`verification/RESULTS.json` had not been rewritten since `0f9348e` — 117 commits. Bisected in git-archive
rooms: `FIRST_RED[flow]=b6457b0`, `FIRST_RED[hosted]=b8b0e71`. Both repaired, and the record republished:

    delivered record written: 71 groups, 4248 checks
    summary: 67 pass, 0 fail, 4 unavailable, 0 absent of 71 groups, 4248 checks

The four UNAVAILABLE rows are missing companion artifacts (`output/pre-branch-controller`,
`output/pre-auth-controller`, `output/previous-controller`, and the candidate APK
`RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk`), not failures.

**Unit anchor corrected: `Rust cases passed: 878/878; isolated application cases: 57`** — 876 at `4309c84`
plus one case from each repair commit. A mid-window note pinned 877, one case short.

**The R-rule debt in the verification tree is now reachable.** `tools/build-verification.sh` compiles each
`verification/<name>.rs` crate root with `$CLIENT_RUST/bin/rustc` (no Cargo, no Python) into
`output/verification/<name>`; the runner prefers that twin for both input derivation and execution, and
falls back to `python3` only when no twin exists. `verification/twin/mod.rs` is the shared helper
(`Controller` over `output/headless/RLLM --jsonl`, a `Value` type, `report`). `verification/history.py` is
gone: verification Python went **73 files / 8570 lines → 72 files / 8547 lines**, and both package ceilings
ratcheted with it (`VERIFICATION_PY_FILE_CEILING` 73 → 72, `VERIFICATION_PY_LINE_CEILING` 8570 → 8547).

**Equivalence is measured, not asserted.** In one room (`/tmp/twincmp`) the retired Python leg and the
native twin ran side by side:

    python leg  rc=0  report sha 5d62f873ad5d875f… 472 bytes  "6 history checks passed"
    native twin rc=0  report sha 5d62f873ad5d875f… 472 bytes  "6 history checks passed"
    report bytes identical: True; counts 6/6; scope identical; order identical

That witness is what forced `c7c670b`: the first twin wrote compact JSON, which reflowed that group's block
in the delivered record. After the fix the record is **byte-identical across the conversion** — the sweep at
`c7c670b` produced sha `e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`, the same bytes
committed at `0eb0cb1`.

**Failure semantics survived the move.** With the twin source broken, the build fails and leaves no stale
binary (`broken_build_rc=1 twin_present=no`), and the leg goes `history FAIL (exit 2 …)` rather than
replaying an old pass; with the twin binary moved aside it goes `FAIL` plus `COUNT UNPARSED: history`; with
a source removed the rebuild prunes its orphan binary. The runner census now also refuses an undeclared
top-level twin (`CENSUS: verification/zz_scratch.rs is not a runner group…`). Tools tests stayed green and
the same size: package 46, harness 23, gate 18, toolchain 10, apk 6, readback 24, checkpoint 5, signature 5,
alignment 2.

**`GAP5` status.** Python exposure measured at `c7c670b`: `verification/` 72 files / 8547 lines,
`tools/` 2 files / 189 lines (`checks.py`, `package.py`), `core/`+`platform/` 0. Total **74 files /
8736 lines**. Eight more legs are drafted as twins by MiniMax writer seats (all Python check names present
in each draft): `mcp_catalog_events` (801 lines), `rrsi_loop` (518), `http_framing` (336), `self_rebuild`
(329), `proposal_recovery` (246), `mcp_continuation` (237), `mcp_catalog` (190), `a2a` (167). The two
largest drafted files are in Grok repair lanes (`/tmp/laneD`, `/tmp/laneE`) because the drafts never saw a
compiler (60 and 65 type errors). **Nothing in that queue is claimed** until it compiles clean and clears
the same two-column bar. `tools/checks.py` retires when the runners point at `gate <units>`; `tools/package.py`
retires when a signed candidate is published, which is still owner-blocked: signing custody needs `rllm.p12`
and `rllm.password`, and the certificate digest stays `ba3d016e…` — no keystore gets minted.

Not claimed: signed-path behaviour (no signed APK on this host), device acceptance, and any `GAP2`/`GAP3`/
`GAP4` capability beyond the run logs already in `docs/`.

### 2026-11-12 — five connected verification legs went native (twin batch 1)

`verification/mcp_history.py`, `http_framing.py`, `rrsi_loop.py`, `mcp_catalog.py` and `proposal_recovery.py`
are deleted; each group now runs a native twin `verification/<name>.rs` that `tools/build-verification.sh`
compiles and the runner prefers. Verification Python went **72 files / 8547 lines → 67 files / 7052 lines**, and
both package ceilings moved with the deletion (`VERIFICATION_PY_FILE_CEILING` 72 → 67,
`VERIFICATION_PY_LINE_CEILING` 8547 → 7052), including every census string pinned inside the package tests.

**Each conversion is certified by a two-column witness, not by the lane that wrote it.** In one room built from
`git archive HEAD` plus the twin, the retired Python leg and the twin ran side by side and their report files
were byte-compared; then the runner ran the group (`/tmp/twinwitness5.sh`):

    group                twin rc / py rc   shared report sha256                        runner row
    mcp_history          0 / 0             19fb92949c89d92e…  mcp_history PASS (exit 0, 0s, 21 checks)
    http_framing         0 / 0             d859d322aa37294d…  http_framing PASS (exit 0, 0s, 189 checks)
    rrsi_loop            0 / 0             c7f2df8a38750090…  rrsi_loop PASS (exit 0, 1s, 15 checks)
    mcp_catalog          0 / 0             f3fe2c46ef251824…  mcp_catalog PASS (exit 0, 0s, 30 checks)
    proposal_recovery    0 / 0             cca9b25d5b1c25ac…  proposal_recovery PASS (exit 0, 8s, 249 checks)

The five legs do **not** share one report shape — the delivered record splices each report verbatim, so a twin
must reproduce its leg's exact JSON: `passed` as a number plus a `checks` list and a trailing newline for four
of them, `passed` + `checks` + `requests` with **no** trailing newline for `mcp_catalog`. That is why
`verification/twin/mod.rs` now carries one writer, `twin::report_value(group, body, trailing_newline)`, instead
of five hand-rolled ones.

**Audits ran and were read before the commit.** Five MiniMax audit seats each received the twin, its retired leg
and the shared helper embedded in the packet. Three findings were real and are fixed in the shared helper rather
than patched per-twin: scratch-directory uniqueness (`twin::create_scratch` reserves the name with `create_dir`
and retries on collision, replacing five PID+nanos `create_dir_all` variants, and it panics instead of
`unwrap_or(0)` on a pre-epoch clock), and report escaping (`quote()` now emits non-ASCII as UTF-16 code units
like Python's `json.dumps(ensure_ascii=True)`, so `proposal_recovery`'s post-write `escape_report_ascii` pass is
gone). Two findings were refuted with measurements: the `mcp_history` claim that stored-DEFLATE IDAT makes "the
PNG fixture not the same subject input" — that leg never inflates the fixture, the PNG travels as a base64
*string* and every predicate counts that string in an outbound wire (leg lines 115–193) — and the `rrsi_loop`
claim that the duplicated sha/zlib codecs "duplicate shared `mod.rs` functionality": `mod.rs` exports no sha or
zlib at all, so the real item is twin-to-twin duplication, which stays open as a consolidation lane. The
`mcp_catalog` re-audit's one surviving finding is labelled non-pass-converting by the seat itself.

**Behaviour-neutral repairs are provable here.** The five report sha256 values are unchanged by those helper
repairs, and the sweep after the repairs wrote a delivered record whose sha256 stayed
`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0` — the same bytes as `HEAD`, i.e.
**71 groups / 4248 checks, 67 pass, 0 fail, 4 unavailable, 0 absent**, with the five groups now running natively.

**Gate on the committed bytes:** `Rust cases passed: 878/878; isolated application cases: 57`; tools tests the
same size (alignment 2, readback 24, checkpoint 5, signature 5, apk 6, toolchain 10, gate 18, package 46,
harness 23) with the 67/7052 pins; six twins compiled with 0 errors and 0 warnings.

**Python exposure now:** `verification/` 67 files / 7052 lines, `tools/` 2 files / 189 lines (`checks.py`,
`package.py`), `core/`+`platform/` 0 — **69 files / 7241 lines**. Next in the pipeline: `mcp_catalog_events`
(lane claim, needs its witness), and `voice` (375), `self_rebuild` (9), `mcp_continuation` (32), `media_stream`
(41), `scene` (90) in or out of Grok repair lanes; the MiniMax draft queue holds the rest, and none of it is
claimed until it clears the same two-column bar. `tools/checks.py` retires when the runners point at
`gate <units>`; `tools/package.py` retires only when a signed candidate is published — signing custody is still
owner-blocked (`rllm.p12` + `rllm.password`, certificate digest `ba3d016e…`, no keystore gets minted).

### 2026-11-12 — five more legs went native (twin batch 2), and the second runner had to follow

`verification/voice.py`, `mcp_continuation.py`, `self_rebuild.py`, `media_stream.py` and `mcp_catalog_events.py`
are deleted in favour of native twins. Verification Python goes **67 files / 7052 lines → 62 files / 5391
lines** (`VERIFICATION_PY_FILE_CEILING` 67 → 62, `VERIFICATION_PY_LINE_CEILING` 7052 → 5391).

Witnessed the same way — one fresh `git archive HEAD` room per group, retired leg and twin side by side,
byte-compared reports, then the runner (twin rc / leg rc / shared report sha256):

    voice              0 / 0  ccc84d628647a859…  375 checks
    mcp_continuation   0 / 0  c7b50e59177c69ce…   32 checks
    self_rebuild       0 / 0  14450bc7ed0b22bc…    9 checks
    media_stream       0 / 0  f21be55521dacbc3…   41 checks
    mcp_catalog_events 0 / 0  0691744165d1b0d1…  145 checks (+ http_requests 193)

Two things this batch measured rather than assumed:

- **`self_rebuild` needs `output/tools/gate`.** In a room without `bash tools/build-tools.sh output/tools`,
  the *Python* leg dies with `FileNotFoundError: … /output/tools/gate` and the runner records the group
  `UNAVAILABLE`; the twin panicked for the same missing input. A witness that skipped the tools build would
  have compared two failures.
- **A report-shape defect can pass.** The `mcp_catalog_events` twin ran 145 checks and the runner printed
  `PASS (exit 0, 0s, 145 checks)`, but its report was `twin::report`'s default `{passed:[names],scope}` while
  the leg writes `{passed:int, checks:[145], http_requests:193}` plus a trailing newline. Only the byte compare
  caught it; lane T fixed it and the re-witness came back byte-identical. Report file names are not simply the
  dashed group name either — `voice`, `media_stream`, `scene`, `snapshot_identity`, `services`, `oauth`, `acp`
  and others keep their stem unchanged.

**Audit gate, five seats, read line by line.** `mcp_catalog_events` was accepted clean. Every other seat's
substantive finding was refuted against the sources: `voice` "resumes `before_stop`, not `stopped['state']`" —
`voice.py:38` resumes from `d['state']`, the pre-stop snapshot; `mcp_continuation` "`.any(route == "/child")` is
weaker" — the leg's own conjunct is `'/child' in [route for route, _ in model_requests]` (line 228);
`media_stream` "upload silently returns without `media_upload`" — the leg imports `transport.upload`, whose line
5 is `if not e.get('media_upload'):return`; `media_stream` "`/created-file` is not POST" — the leg's route dict
uses `method='POST' if response=='artifact_url' else 'GET'` (line 45) and that case is `response='binary'`.

**The deletions exposed a second runner that could only run legs.** `tools/checks.py` built every connected
group command as `python3 verification/<name>.py`, so after the deletions it would have re-run a stale leg or
failed. It now prefers `output/verification/<name>` exactly as `tools/harness.rs` does. Its group list stays
pinned against the runner's `GROUPS` list (`assert runner == names`), which is the check that keeps the two
sanctioned runners enumerating the same 71 groups.

**Gate on these bytes:** tools tests unchanged in size (package 46, harness 23, gate 18, toolchain 10, apk 6,
readback 24, checkpoint 5, signature 5, alignment 2) against the 62/5391 pins; 11 twins compile with 0 errors
0 warnings; sweep delivers **71 groups / 4248 checks, 67 pass / 0 fail / 4 unavailable / 0 absent** and
`verification/RESULTS.json` stays byte-identical to the batch-1 record
(`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`).

**Python exposure now:** `verification/` 62 files / 5391 lines, `tools/` 2 files / 191 lines, `core/`+`platform/`
0 — **64 files / 5582 lines**, down from 74 / 8736 when this route started. Still open in the pipeline:
`scene` (90) and `snapshot_identity` (61) and `a2a` (72) in lanes, the codec consolidation across
`mcp_history.rs`/`http_framing.rs`/`rrsi_loop.rs` in a lane of its own, and the remaining 62 legs behind the
draft queue.

## 2026-11-12 (late) — one codec implementation replaces three private copies

The three twins that needed content codecs each wrote their own: SHA-256, SHA-1, CRC-32, Adler-32, stored
DEFLATE, zlib, gzip and a full inflate. `verification/twin/mod.rs` now carries that code once
(`sha256_bytes`, `sha256_digest`, `sha1_digest`, `crc32`, `adler32`, `deflate_stored`, `zlib_compress`,
`gzip_store`, `zlib_decompress`, `inflate_zlib`, `inflate_deflate`, `inflate_block`, `inflate_blocks`,
`gzip_decode`, `skip_gzip_text`) and the twins call it: `mcp_history.rs` 871 → 832 lines, `http_framing.rs`
1248 → 1215, `rrsi_loop.rs` 2036 → 1602 — **506 duplicated lines removed**, helper 649 → 1246.

**A consolidation is only behaviour-neutral if the digests say so.** Re-witnessed in a git-archive room over
the staged files (`/tmp/rwu.log`): 11 twins built, 0 errors 0 warnings, and for all three groups twin rc=0 /
leg rc=0 / `cmp` byte-identical with the report sha256 **unchanged** from the pre-consolidation twin
(`19fb9294…` 21 checks, `d859d322…` 189, `c7f2df8a…` 15). The digest not moving is the proof; a report that
still "passes with the right count" would not be.

**Three audit seats, read line by line.** `mcp_history` accepted with no defect. `rrsi_loop`'s three findings
were all refuted against the leg: the `round_note`→`notice` fallback is the leg's own line 246
(`decision=proposed_run.get('round_note') or proposed['state'].get('notice') or ''`); accepting a missing
`workspace` matches `compact_run.get('workspace')` (line 171) asserted by `compact_workspace in (None,'')`
(line 180); and the error-tail `unwrap_or_default()` sits in the `Ok(0)` stdout-closed branch that panics
either way, mirroring `rrsi_loop.py:112`. `http_framing`'s two findings are both **pre-existing at HEAD**
(`py_bytes`, `finished_rx.try_recv().unwrap_or(Ok(()))`), so they are outside a commit whose whole claim is
digest equality. `py_bytes` as a "duplicate helper" is refuted — it is defined once in the tree and two check
names embed its Python-`repr` output byte-for-byte. The `try_recv` default is a real weakening (a server
thread that dies before reporting reads as success) and is carried as a repair item.

**Gate on these bytes:** tools tests unchanged in size (package 46, harness 23, gate 18, toolchain 10, apk 6,
readback 24, checkpoint 5, signature 5, alignment 2) against the 62/5391 and 191 pins; 11 twins with 0 errors
0 warnings; sweep **71 groups / 4248 checks, 67 pass / 0 fail / 4 unavailable / 0 absent**;
`verification/RESULTS.json` still
`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`. Python exposure is unchanged by this commit
— it is a Rust-side dedup, not another leg retirement.

## 2026-11-12 (night) — twin batch 3: four more legs native, and the helper's own tests get a build row

`verification/scene.rs` (1763 lines) replaces `scene.py` (110), `a2a.rs` (1033) replaces `a2a.py` (167),
`snapshot_identity.rs` (828) replaces `snapshot_identity.py` (87), `voice_session.rs` (298) replaces
`voice_session.py` (79). The legs are deleted because the runner prefers the twin.

**Python exposure: 62 files / 5391 lines → 58 / 4948 in `verification/`; `tools/` stays 2 / 191 and
`core`+`platform` stay 0. Total 64 files / 5582 lines → 60 files / 5139 lines. Native twins 11 → 15.**

**Each group certified by a two-column witness over one tree** (git-archive room; twin rc, leg rc, `cmp`
byte-identical and both report digests printed by the same instrument): `scene`
`01792939b338a96d16a65a9daa68304e85fcb8e3effb9fdfdda053e90b5e5c86` (90 checks), `a2a`
`77e60a517c23876805295ae05d4c651956a1d02bc6b2fbb5d8ec9944c03d3e83`, `snapshot_identity`
`5f26f895a81392de705a4594464c9f83b90f08a868a4b21f82f0b1ef555a0f2a`, `voice_session`
`420f1a258ed008e91a8e881420387c01650ffde327928524223bfcff8ac1e6ea` (34). The `a2a` and `snapshot_identity`
witness rows printed `COUNT UNPARSED`, so their counts come from this commit's sweep rows (72 and 61) and from
the leg reports `cmp` matched — not from a lane's say-so. The scene `--previous-scene-controller` arm (4 extra
checks) is implemented in the twin but is not what the delivered record runs.

**The audits found real gaps.** `scene` was missing the previous-controller arm entirely and rolled its own
scratch root (now `twin::create_scratch`). `a2a` carried a private `sha256`, a private `truthy` and libc
`kill()` lines — now `twin::sha256_bytes`, `twin::truthy`, std `Child::kill`, keeping one libc `SIGTERM`
because that is what the leg's `terminate()` sends; it also swallowed absent fields with defaults where the leg
raises `KeyError`, and now panics. `voice_session` ran the Java subject through an unbounded
`Command::output` while the leg bounds it with `subprocess.run(timeout=20)` — a hung subject would have hung
the twin rather than failed it — so it now polls `try_wait` against a 20-second deadline. Refuted:
`snapshot_identity` "subject substitution is weaker" (same marker/slice/replacen; the marker is at
`Activity.java` 621/889/926 and both take the first), `scene` "single_again" / "hardcoded channel" / "child not
reaped" (`std::try_wait` performs the wait), `a2a` "`unwrap_or_default` weakens output/compare" (both checks
require containment, so `""` fails), `voice_session` "names/order differ" and "passed count fabricated"
(byte-identical `cmp`; names zipped by index against the Java program's own emitted names). Two drafts also
arrived **exactly one byte short** of their leg's report — a missing trailing newline — which a count check
would have passed; only `cmp` caught it.

**`twin::truthy` is new, and it came from a product-shape mistake.** The acp twin went red because it compared
`promptCapabilities` against `Null`, while `core/acp.rs:55` advertises `{}` and `loadSession: false`; the leg
asserts Python falsiness (`not cap['promptCapabilities']`). A twin that compares against `Null` rejects the
empty object the product actually emits. The helper now reproduces Python truthiness over the shared `Value`
type, with a unit test, and `a2a` uses it instead of its own copy.

**A proof source with no build row never runs.** `verification/twin/mod.rs` had 8 `#[test]` cases and nothing
compiled them — dead source that still read like proof. `tools/build-verification.sh` now compiles the module
with `--test`, runs it single-threaded, and fails the gate unless the executed count equals the declared count
(measured: `twin helper tests: … cases: 8`).

**Gate on these bytes** (`/tmp/gate-batch3.log`): tools tests unchanged in size against the moved pins
(verification 58 / 4948, tools 191); `build-verification` rc=0 with **15 twins, 0 errors 0 warnings**; sweep
**71 groups / 4248 checks, 67 pass / 0 fail / 4 unavailable / 0 absent**, the four new rows PASS at
72 / 61 / 34 / 90 checks; `verification/RESULTS.json` still
`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0` — byte-identical to the batch-1, batch-2 and
consolidation records, which is what proves four legs were replaced rather than re-scored.

## 2026-11-12 (late night) — twin batch 4: eight more legs native, and the runner learns to count a twin

**Eight legs retired in one batch:** `verification/acp.rs` (555) replaces `acp.py` (81), `oauth.rs` (1797) replaces
`oauth.py` (100), `services.rs` (801) replaces `services.py` (101), `dav_listing.rs` (583) replaces
`dav_listing.py` (106), `server_admission.rs` (279) replaces `server_admission.py` (28), `state_sealing.rs` (207)
replaces `state_sealing.py` (40), `desktop_runtime.rs` (328) replaces `desktop_runtime.py` (43),
`capture_lifecycle.rs` (166) replaces `capture_lifecycle.py` (42). Python exposure in the index goes
**verification/ 58 files / 4948 lines → 50 / 4407**, total **60 / 5139 → 52 / 4598**, with **23 native twins**.
`tools/package.rs` pins moved by the ratchet (`50 / 4407`) and `tools/checks.py` needed no group-list edit — it
pins its list against the harness's `GROUPS` constant and already prefers a compiled twin.

**Each group certified by a two-column witness over one tree** (git-archive room; twin rc, leg rc, `cmp`
byte-identical, both digests printed by the same instrument, then the per-group harness row for the count):
`acp` `4715017b71654f06fb0afe0f9d0f1b9be13728d8408645154ac04816e5a5e659` (41), `oauth`
`65aab67c91e0a06626722823ee167806ff9f0314b8f4d89b443ed95558f1af72` (121), `services`
`0b93f1b8ce997f1d3a71c07f2a68ac1d6164e610b950ffaa4c2e5cb41c3ac164` (72), `dav_listing`
`9f8b2b4a9cbf779b78c48a218405dee8dab4ba2866e02f67bcf824a28a735e2c` (46), `server_admission`
`971ab53ca697db57795a80128185c5ebeef1bf26d8895dd7608ce93efdc2c1c3` (8), `state_sealing`
`8bbd78de7e99bfa0af1953c3109344d15143652e55a86c2be705b48a546ef2dd` (16), `desktop_runtime`
`3d699651eece754a2fd01eef8bc199150f4b17f71c2b55b9707ea5620366872b` (24), `capture_lifecycle`
`90c04b8f12396a4faf34d622f0782ee03b73ee95140465af81bae024232da829` (15). `acp`, `oauth`, `services`,
`desktop_runtime` and `capture_lifecycle` were then **re-witnessed after the accepted audit fixes and every digest
stayed byte-identical** — the proof that the fixes changed fixture fidelity, not the verdict.

**The runner could not count a twin at all, and the witness rows showed it.** `output/harness <group>` harvests a
count from a stdout JSON object, a `"<N> <group> checks passed"` line, or a report file whose name is a string
literal quoted in the group's *source*. A Python leg quotes its report path; a twin derives the filename from
`verification/twin/mod.rs::group_report_file(group)`, so the name appears nowhere in its source and the rows read
`services PASS (exit 0, 0s, unparsed checks)`, `COUNT UNPARSED: oauth` — in the same rooms where `cmp` proved the
reports byte-identical. The sweep path never had this hole (it derives the path itself), which is why the record
was right while the instrument was blind. Fix: `harvest` now passes `group_report_file(name)` into `file_measure`
as a canonical name. A stricter variant — require `mtime >= run start` — was tried **and measured unusable**: this
filesystem truncates modification times, so a file written *after* the run started still reads older than the
start. That is what the harness's two-second window is for, and why provenance now comes from `clear_report`:
the expected report file is deleted before the group runs, so its presence afterwards is the evidence, and a
report left by an earlier run can no longer stand in for an execution that did not happen.

**The audits found real gaps.** `capture_lifecycle` ran its Java subject with an unbounded `Command::output` while
the leg bounds it `subprocess.run(timeout=15)`, and swallowed a `remove_dir_all` failure where the leg's
`TemporaryDirectory` context manager lets a cleanup failure raise after the body — both fixed (15-second
`try_wait` deadline with kill, cleanup failure panics). `acp` defaulted a missing `rawOutput` output where the leg
raises `KeyError` and carried a private 55-line SHA-256; `oauth`'s fixture peer accepted an authorization request
missing `code_challenge`/`state`/`redirect_uri`/`client_id` and defaulted them, counted connections instead of
requests, and mapped an undecodable `%xx` escape to a NUL where `urllib` keeps it literal; `services` defaulted a
missing effect host; `desktop_runtime` used an unbounded host wait against the leg's `timeout=180`, duplicated the
helper's scratch reservation, and carried a dead `_duration`. **Refuted:** `services` findings that the twin reads
the wrong peer (`Peer::pair` gives both fixture peers ONE shared request log, exactly like the leg's single
`requests` list) and that the leg asserts a 503 first response; `oauth` findings calling the fixture's controlled
model bodies "fabricated" (the leg's own peer answers from the same table); `acp` "extension values must be
truthy" (the leg checks membership only); `desktop_runtime` "drops the inherited environment", "final non-empty
stdout line", "truncates to fewer than 24 bytes". The harness instrument's own audit was refuted on two points:
the `count_source` label it feared would move the delivered record does not exist in `commit_sweep`'s `SweepRow`
serialization, and the mapping exceptions only newly *conflict* when two fresh reports genuinely disagree.

**Gate on these bytes** (`/tmp/gate5.log`, `/tmp/gate6.log`): units `Rust cases passed: 878/878; isolated
application cases: 57`; `build-tools` rc=0 with tool tests alignment 2, readback 24, checkpoint 5, signature 5,
apk 6, toolchain 10, gate 18, package 46 and **harness 23 → 27** (the four new count-source tests);
`build-verification` rc=0 with **23 twins, 0 errors 0 warnings** and `twin helper tests: … cases: 8`;
`bash tools/build-harness.sh` rc=0 (`output/harness` sha256
`8619f171b8a9b5e5063e8cecb1ae4731f1dd83e62ececa0902eea58070999426`) — that step had to be added to the gate chain,
because `build-harness.sh` is what builds the runner and a chain that skips it re-measures with a stale binary: the
first post-fix rows still read `unparsed checks` for exactly that reason; `bash headless.sh` rc=0; single-group
rows on the fresh runner now counted: services 72, oauth 121, acp 41, dav_listing 46, server_admission 8,
state_sealing 16, capture_lifecycle 15, desktop_runtime 24, a2a 72, scene 90; sweep **71 groups / 4248 checks,
67 pass / 0 fail / 4 unavailable / 0 absent**, with `verification/RESULTS.json` still
`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0` — byte-identical through batches 1–3, which is
what proves eight legs were replaced rather than re-scored, and that an instrument change moved no verdict.

## 2026-11-13 (early) — twin batch 5: eleven more legs native, one controller transport, and the printed-report contract


**Eleven legs retired:** `flags_flow` (593), `voice_background` (871), `provider_evidence` (1164),
`device_authorization` (1104), `delegation_visibility` (1380), `carry_flow` (519), `platform_resolution` (140),
`response_fields` (407), `review_ownership` (622), `storage_etags` (546), `jobs` (953) — 8 299 lines of Rust replacing
781 lines of Python. Python exposure in the index: **verification/ 50 files / 4407 lines → 39 / 3626**, tools/ stays
2 / 191, **total 52 / 4598 → 41 / 3817**. Native twins **23 → 34**. `tools/package.rs` ceilings ratcheted with the
census (tests pin `39` / `3626` exactly, and the refusal-message fixtures moved with them).

**The shared helper did three jobs in this batch.** (1) `Controller::spawn_at(binary, root)` was added and
`Controller::spawn` now delegates to it, after the `flags_flow` audit caught the twin carrying a 69-line private
copy of the controller transport (`struct Published` + `impl` + `Drive`) whose only difference was launching
`output/published-controller --jsonl` — the same framing the leg drives at `flags_flow.py:91`. The twin is 663 → 593
lines and drives both subjects through one transport. (2) `Value::Obj` equality became **two-directional**: the old
rule matched every pair of the left object somewhere on the right with equal lengths, so a repeated key could stand
in for a different key (`{"a":1,"a":1}` == `{"a":1,"b":1}`). The existing helper test now pins that pair in both
orders; deleting the reverse match makes it fail (7 passed, 1 failed), measured on the mutant. Both changes are
proven inert on the delivered record: `output/harness sweep` with the new helper still writes
`71 groups, 4248 checks / 67 pass, 0 fail, 4 unavailable, 0 absent` and `verification/RESULTS.json` stays
`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0`. (3) The third helper job was not inert — it
repaired a red. `report_value` now composes `report_text` (pretty body + optional trailing newline) over
`report_path`, and `report_value_printed` writes that text to the report file **and** prints the identical bytes,
because two legs report on stdout instead of writing a file and the runner counts the printed object. Its twin test
pins the printed byte form against the leg's own formatter (`json.dumps(body, indent=2)` plus `print()`'s newline),
measured, so indent or newline drift goes red in the helper row rather than in a sweep.

**Witness rows (two-column, one tree, twin vs the Python leg).** `voice_background`
`b69a5ec32fbea0ed916d7cf00b94f8f1676ebf42480f98337621b4c74acaa27d` (52), `provider_evidence`
`2646816090ad91ccf8e12e36a85ac3f1dd09a5ad4854a29a4db30a028f6894d6` (40), `device_authorization`
`70736ae14a0e5232d142aa485acf47fedbd23db0839a3e22f9eee8171aee81e1` (92), `delegation_visibility`
`c0840d6a07e275b0d9f6db76e48dae00a3dd33ab77e89f0a0b4c36a2c8733955` (94), `carry_flow`
`13eb5eea8011e976323555ca3f36d5b449b1409f8725565e4f00c7334510d7f5` (23), `review_ownership`
`33bd77bca0c9578a969683f928c5fefdc365b788d1946b6c3a8b7f1e8301a857` (28) — each byte-identical to the leg's report,
each with a counted harness row. `flags_flow` `7217b0c500336f592a55149d583abb6dfed88a4eb4e995ade20700ac` (28) was
certified before the transport dedup and re-witnessed after it; `platform_resolution` failed its first witness —
`twin-report.json` ended at byte 794 where the leg writes a trailing newline (`json.dumps(result, indent=2)+'\n'`),
and the twin spawned a headless controller the leg never starts — fixed by writing the body with
`report_value(…, true)` and dropping the unused controller (140 lines, 0 warnings), then re-witnessed.

**Audit seats read line by line; the accepted findings are in the bytes.** `platform_resolution`: report shape and
an unnecessary subject spawn (fixed). `flags_flow`: the duplicated controller transport (fixed).
`gateway_expectation` (queued next batch): a present-but-unparsable `Content-Length` became `0` where the leg's
`int(...)` raises — now a panic, while a genuinely absent field keeps the leg's own `0` default. **Refuted against
the legs:** `voice_background` (5), `device_authorization` (3), `delegation_visibility` (1), `jobs` (2 — the seat
condemned mutants it proposed, e.g. twin:107 already compares `If-Match` bytes exactly like `jobs.py:47`),
`review_ownership` (2 — it read `run(controller, root)`'s parameter as the project root; the caller binds the scratch
directory, exactly as the leg's `root=pathlib.Path(tmp)`), `storage_etags` (3 — `only_original()` already pins
`files=={b'/share/file.txt': b'old text'}` like `storage_etags.py:51`), `carry_flow` (3 — the leg's root is the
scratch dir and its predicate is the same `effect.wire` substring test), `question_process` (1 — Python's
`c.get('inquiry_id') != b` is True for a missing key, matching the twin's `None => true`), `request_fields`
(1 — the seat's own text concludes the default cannot turn a failure into a pass). `response_fields` and
`platform_resolution` audits returned full leg-order coverage tables with mutation analysis and accepted.
`provider_evidence` needed three seats before one read the whole 1 164-line file; the two half-file seats produced
11 findings that were artifacts of the split (helpers defined in the other half), all refuted against
twin:1049-1164.

**Gates, in order, on the committed bytes.** `bash tools/build-units.sh output/units` rc=0 and
`python3 tools/checks.py --all-units output/units` rc=0 (878/878 cases, 57 isolated application cases).
`bash tools/build-tools.sh output/tools` **rc=1** the first time: the package census test plants its corpus from
the ceiling constants, so after the ceilings moved to 39 files / 3 626 lines the planted tree printed
`verification files 39 lines 39 ceiling files 39 lines 3626` while the assertion still expected `files 50 lines 50`
(`tools/package.rs:4152`). The expectation moved with the constants and the fixtures; the rerun
(`/tmp/g8b-tools.log`) is rc=0 with `package-tests cases: 46` and every other tool row at executed == declared
(alignment 2, readback 24, checkpoint 5, signature 5, apk 6, toolchain 10, gate 18, harness 27).
`bash tools/build-verification.sh .` rc=0: 34 twins, 0 errors, 0 warnings, `twin helper tests: … cases: 8`.
`bash tools/build-harness.sh` rc=0 (`output/harness` sha256
`8619f171b8a9b5e5063e8cecb1ae4731f1dd83e62ececa0902eea58070999426`), `bash headless.sh` rc=0.

**The sweep went red on a real contract, not on a check.** `response_fields FAIL (exit 0, 0s, absent checks)` and
`delivered record left untouched: a group failed` — `summary: 66 pass, 1 fail, 4 unavailable, 1 absent of 71 groups,
4203 checks` — while the same group counted 45 checks in single-group mode. The two legs named in
`tools/harness.rs:1661` (`response_fields`, `chunk_fields`) print their report JSON and write no file, so the
sweep counts and persists the printed bytes (`persist_printed_json`, `tools/harness.rs:1285-1293`); the twin
printed a human summary line and wrote the file instead, so the sweep had no report text to count. The shared
helper gained `report_value_printed` (one `report_text` for both columns) and the twin now calls it; the 9th
helper test pins the printed byte form to `json.dumps(body, indent=2)` plus `print()`'s newline, measured from
the leg's own formatter. Rebuild: `twin helper tests: … cases: 9`, 0 errors / 0 warnings.

**Printed-group witness (`/tmp/witq17.log`), both columns the runner can read.** The twin's stdout and the leg's
stdout are byte-identical at `160aedcfc972e80166eee4e7f2d34cb38c7ae96c24e6703ef22dd3b304b83928`; with the twin
binary parked so the leg actually runs, the file the runner persists from the leg's stdout is byte-identical to
the file the twin writes, same digest; the leg wrote no report file, as expected; `response_fields PASS
(exit 0, 0s, 45 checks)`.

**Final gates on these bytes.** `output/harness sweep` rc=0: `delivered record written: 71 groups, 4248 checks`,
`summary: 67 pass, 0 fail, 4 unavailable, 0 absent of 71 groups, 4248 checks`, and `verification/RESULTS.json`
still `e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0` — byte-identical through batches 1–5,
which is what proves eleven legs were replaced rather than re-scored. A sweep run without sourcing
`tooling/env.sh` printed 8 rows of `exit 101` (`CLIENT_JDK`/`CLIENT_NDK`/`CLIENT_RUST` unset) and left the record
untouched: an instrument-use error, recorded here so it is not mistaken for a product red. Census after this
batch: `verification/` 39 `.py` / 3 626 lines and 34 native twins, `tools/` 2 `.py` / 191 lines — 41 files /
3 817 lines, against ceilings 39 / 3 626.

## 2026-11-13 (morning) — twin batch 6: seventeen more legs native, and the witness becomes two-column

**What went native.** Seventeen connected legs were replaced by native twins and their Python deleted with
`git rm`: gateway_expectation 422, question_process 603, request_fields 579, voice_interruption 1 105, media 965,
proposal_feedback 950, network_feedback 928, document_providers 530, storage_owner 1 212, provider_process 670,
chunk_fields 485, permission_modes 686, field_flow 515, storage_auth 761, audio_route 212, device_shortcut 444,
voice_settings 1 029 — **12 096 lines of Rust against 1 259 lines of Python**. `package.rs` ceilings moved with
them: `VERIFICATION_PY_FILE_CEILING 39 -> 22`, `VERIFICATION_PY_LINE_CEILING 3626 -> 2367`, with the planted
corpora and refusal sentences in the same edit. Census after: `verification/` 22 `.py` / 2 367 lines (including
the shared `verification/transport.py`, still imported by seven legs) and **51 native twins**; `tools/` 2 `.py` /
191 lines — 24 files / 2 558 lines.

**The witness standard, because an `ls -t` pass proves nothing.** Every group is certified in a fresh
`git archive HEAD` room by `/tmp/twinwitness2.sh`, which derives the compared report path from the runner's own
`tools/harness.rs:862-875 group_report_file` mapping — never file recency — and logs both instruments' exit codes,
both artifact sets (`twin_files=… | all=…`, `py_files=…`) and both report digests before `cmp`. Passes 44, 46-48,
50, 52-57 are clean under that instrument; the older passes it replaced are flagged in the table. Final certified
set (group, checks, report sha256, pass): provider_process 60 `2681e47a…` 34; storage_auth 55 `1240b49b…` 42;
gateway_expectation 26 `931298af…` 50; request_fields 55 `6510cbda…` 38; device_shortcut 240 `926a0b06…` 39;
question_process 19 `f1d2a5b6…` 57; network_feedback 41 `a1e2fcfb…` 47; voice_interruption 104 `a638a8ee…` 54;
proposal_feedback 36 `ac8e6a4f…` 46; field_flow 37 `1891092a…` 41; media 33 `0652284c…` 56; chunk_fields 163
`f2a92580…` 28; permission_modes 27 `e766f33e…` 29; audio_route 23 `b6299737…` 32; document_providers 51
`a877bf7f…` 52; storage_owner 43 `5ef1f183…` 53; voice_settings 81 `d4f2b280…` 55. Each row is byte-identical
between the twin and the leg, and each twin wrote exactly the canonical report and nothing else.

**What measurement found that reading did not.** `voice_settings` hung (pass 36 `twin_rc=124`, no canonical
report): `/proc` showed the child blocked in `anon_pipe_read` while its sockets sat established with empty
queues — the twin never ran `verification/transport.py`'s protocol (upload-read loop, per-8192 stream feeds,
finish with `eof`, both cancels) on the still-open socket, and it invented the `network_result` inputs instead of
the leg's `dict(reply, purpose=…)`. Three more real defects fell out of the same loop: three events used
`peer.root()` (`.`) instead of the one scratch root the subject keys pending results to; the model-format
fixtures nested their bodies under the format name, so the subject answered `Missing choices`; and the peer
echoed the `/speak` request body instead of a WAV, which `core/media.rs:51` rejects
("Artifact container differs from configured kind"). `question_process` invented a `Value::Null` plan where the
leg's `old_plan=c['plan']` raises — hardened to a panic, and the report digest did not move
(`f1d2a5b6…` at 19 checks before and after).

**Audit gate.** Eight MiniMax seats ran the full four-block check (coverage / mutations / pass-conversion census /
verdict) and the coordinator read every one, adjudicating each finding against the leg bytes. Accepted and
refuted-with-citations: storage_owner seat 1 rejected two arms for "accepting an absent field", but
`get(k).is_some_and(|v| !truthy(v))` is FALSE when the key is absent, so the twin panics exactly where
`not listed['state']['profiles']` raises `KeyError`; seat 2 re-derived both arms and accepted. question_process
seat 3 rejected `twin:170` for "fabricating `evidence_empty = true`" — that is its own mutation, not the code
(`matches!(…, Some(Arr(items)) if items.is_empty())`), and its census row for the same line reads "No".
`gateway_expectation`, `storage_auth`, `voice_interruption` each had one permissive arm hardened (`eof()` accepts
only a zero-byte read, `as_state` panics instead of substituting, `purpose` goes through `require_clone`), each
followed by a fresh canonical witness.

**Gates, in order, on these bytes.** `bash tools/build-units.sh output/units` rc=0 and
`python3 tools/checks.py --all-units output/units` rc=0 — `Rust cases passed: 878/878; isolated application
cases: 57`. `bash tools/build-tools.sh output/tools` was **rc=1 the first time**: the census byte-bound fixture
still expected `verification files 1 lines 1 ceiling files 39 lines 3626` (`tools/package.rs:4120`); the
expectation moved with the constants and the rerun is rc=0 with all nine rows at executed == declared
(alignment 2, readback 24, checkpoint 5, signature 5, apk 6, toolchain 10, gate 18, package 46, harness 27).
`bash tools/build-verification.sh .` rc=0: 51 twins, 0 errors, 0 warnings, `twin helper tests: … cases: 9`.
`bash tools/build-harness.sh` rc=0, `bash headless.sh` rc=0. `output/harness sweep` rc=0:
`delivered record written: 71 groups, 4248 checks`, `summary: 67 pass, 0 fail, 4 unavailable, 0 absent of 71
groups, 4248 checks`, and `verification/RESULTS.json` still
`e2f35542e44b55094558a8ebcedbbe32f539a4188cdb2b69e84b5de2ab6435a0` — byte-identical through batches 1-6, which is
what proves seventeen legs were replaced rather than re-scored. The four UNAVAILABLE rows are the same four
comparator-bound groups (binary_flow, history_migration, apk_alignment, turn_background); none was retired.

## 2026-11-13 (midday) — the last Python runner in tools/ retires, and its invariants go native

`tools/checks.py` (74 lines) is deleted. It was a cross-check between the Python unit-list document and the
native gate, and its remaining roles were a pinned-filter drift check, byte-level format pins, a selected-case
count pin and a Python-census ceiling. None of those needed Python: the delivered record
`verification/RESULTS.json` and `verification/RESULTS-FORMAT.md` already state the same contract, so a
retired cross-check is a stronger statement than its duplication.

### Before deleting it, one instrument had to witness the other

The two-column witness for `tools/checks.py --all-units` against `output/tools/gate output/units`, on a clean
`git archive HEAD` room with symlinks to the source checkout, not `output/` or `.git`:

```
python3 tools/checks.py --all-units      rc=0  878/878 + 57 isolated
output/tools/gate output/units           rc=0  878/878 + 57 isolated
cmp output/checks/full-units.json        identical, sha256 cb2fd261dc385e7ffb3db408e76a922aa702f17d985fecf0e02ca793923c467c
output/tools/gate --units output/units   rc=0  Selected cases: 310
```

The deleted row pinned those exact bytes, so the native gate had to produce them alone. It does: the same
report path, the same digest, both exit codes 0. A lane report is never a witness — this was re-measured in
the room after the deletion, not inferred from the tree.

### What changed

- `tools/gate.rs`: the pinned-filter drift check now compares the pin against the delivered record
  (`pinned_filters_are_recorded_selected_unit_filters`), because `merge_pinned` unions the two lists and the
  record is the surviving authority on which filters the project treats as selected-unit work. The byte pins
  keep their golden expectations with an honest name (`selected_report_bytes_match_the_golden_document`) and a
  comment that says they pin renderer escaping/key order/indentation/trailing newline, not a Python document.
- `tools/harness.rs`: new test `delivered_record_selects_exactly_the_harness_group_list` gives the runner's old
  two-sided group cross-check a native home — it compares the `counts` object's key order in the delivered
  record against `GROUPS`, which is exactly what the deleted script asserted.
- `tools/package.rs`, `tools/package.py`, `tools/build-units.sh`: census expectations now count one tools
  Python file (`tools/package.py`, 115 lines) and one Python row; the new package test asserts that single
  allowlist entry.
- `docs/BUILD.md`, `verification/README.md`, the harness skill: the `checks.py` route is gone, and the measured
  numbers are re-stated.

### Gates, on the retirement itself

Clean room `/tmp/room-retire` = `git archive HEAD` + these nine files + `rm tools/checks.py`:

```
build-tools.sh          rc=0  9 rows: alignment 2, readback 24, checkpoint 5, signature 5, apk 6,
                               toolchain 10, gate 18, package 46, harness 28 (was 27)
twin helper tests       cases: 9
build-verification.sh   rc=0  51 twins, 0 compile failures
build-harness.sh        rc=0
units gate              rc=0  878/878, isolated 57, full-units.json sha cb2fd261…
gate --units            rc=0  Selected cases: 310
```

`python3 tools/package.py` exits 1 with `FileNotFoundError: output/checks/apk.json` in this room. That is not a
regression from this change: the identical failure was reproduced at HEAD bytes, with `tools/checks.py`
present, in a separate room — `package.py` needs a signed candidate APK artifact that a bare room does not
carry. It is recorded as a missing prerequisite, not a pass.

### Audit disposition

The seat audit read the diff and returned **ACCEPT-WITH-FINDINGS**:

- **F2 (fixed natively):** deleting `checks.py` removed the only enforcement of "the runner's group list
  equals the delivered record's group set". `tools/harness.rs` now asserts it; harness-tests went 27 → 28.
- **F1, F3 (fixed in text):** the pinned-filter comment and the byte-pin test name both claimed they were
  guarding the Python document. They were not, and are now named for what they actually pin.
- **F4, F5 (accepted):** the record-vs-pin comparison is one-sided (`merge_pinned` unions), and the golden
  bytes are renderer fixtures rather than a live cross-check. Both are consequences of retiring a duplicate
  instrument, and are stated in place rather than papered over.

`closure_ledger.json` gained `tools_checks_py_retired_2026_11_13` with this evidence, and the two older keys
that cited `tools/checks.py` as live (`runner_inventory_python_ceiling_zero_slack_2026_11_13`,
`verification_twin_batch1_five_legs_2026_11_12`) were corrected in place;
`checks_py_roles_census_2026_11_13` records that the census was closed the same day it was taken.

## 2026-11-13 (afternoon) — twin batch 7 opens with `local_feedback`, and the ceiling moves again

`verification/local_feedback.py` (71 lines, 29 checks) is replaced by `verification/local_feedback.rs`
(852 lines). It is the first leg of batch 7, so it is committed on its own witness rather than waiting for
the batch.

### The witness, and what it took to earn it

Two-column witness in a clean `git archive HEAD` room (`/tmp/wit84-local_feedback`):

```
compile_rc=0 errors=0 warnings=0        twin_rc=0        py_rc=0
output/checks/local-feedback.json      twin 59af1fafd5399747e10b5737b8c223299efaec325455fe2ef8ff25e9af2ccff0
                                        leg  59af1fafd5399747e10b5737b8c223299efaec325455fe2ef8ff25e9af2ccff0
CMP: byte-identical
harness_build_rc=0   local_feedback PASS (exit 0, 0s, 29 checks)
```

Three divergences had to be found before that equality meant anything, and each is now a rule in the packets
this batch sends out:

- `x['effect'] is None` in the leg means a **present** JSON null. `x.get("effect").is_none()` is false for
  that, so nine sites became `matches!(x.get("effect"), Some(Value::Null))`. The failing twin check was
  `consumed worker callback is inert on replay`, and an instrumented room copy proved the subject emits
  `"effect": null` with `state_equal=true`.
- The leg's nested setter indexes into **arrays** (`st['agents'][0]['tools']`). The twin's helper recursed on
  a string key and treated arrays as objects, looping forever on `["0", rest[0]]` and then panicking
  `expected object`. The rewrite adds a `Value::Arr` arm that indexes by `key.parse::<usize>()`.
- The cancellation check fed the **post**-cancel reply into the failure callback. The leg fails the
  pre-cancellation `copy.deepcopy` snapshot, so the twin keeps that snapshot and compares against it — the
  twin had been testing a different, already-revoked callback.

The report shape also differs from the helper default: this leg writes `json.dumps(...) + '\n'`, so the twin
builds its own `{passed: [...], scope}` body and calls `twin::report_value("local_feedback", &body, true)`
instead of `twin::report`, which writes without the trailing newline.

### Audit disposition

The seat audit read the leg, the twin and the helper block and returned **ACCEPT**: a 29-row coverage table
(leg check → twin line → evaluated predicate, no merge/rename/reorder), three mutations — one twin-value
change that turns the twin red while the leg stays green, one report-byte change, and one inert control that
must stay green — and a pass-conversion census with no site that invents a value the subject did not produce.

### Census

`VERIFICATION_PY_FILE_CEILING` 22 → 21 and `VERIFICATION_PY_LINE_CEILING` 2367 → 2296, with the planted
fixtures in the same edit so they go red first: `publish_refuses_verification_python_over_ceiling`,
`publish_refuses_verification_python_lines_over_ceiling` (2297 vs 2296),
`publish_passes_verification_python_lines_at_ceiling`, `publish_python_census_byte_bound_is_two_sided`,
`publish_passes_compliant_python_census`, `publish_python_census_counts_move_when_a_file_is_added` and
`project_python_census_counts_this_tree`. `verification/README.md` now runs the group as
`output/verification/local_feedback`.

## 2026-11-13 (late morning) — five owner-run legs go native in one batch, and the ceiling drops to 16 files / 1813 lines

`storage_oauth`, `task_ownership`, `drive_ownership`, `credential_rotation` and `media_upload` now run as
native twins. `verification/*.rs` is 57 files; the Python census falls from 21 files / 2296 lines to
**16 files / 1813 lines** (483 leg lines removed: storage_oauth 145, credential_rotation 128,
media_upload 102, task_ownership 61, drive_ownership 47).

### The five witnesses, each a two-column run in its own `git archive HEAD` room

| Group | Room | twin / leg exit | Canonical report (from `tools/harness.rs group_report_file`) | Runner row |
|---|---|---|---|---|
| storage_oauth (78) | `/tmp/witq94-storage_oauth` | 0 / 0 | `output/checks/storage_oauth.json` `b12988860a8b8fb9…` — `CMP: byte-identical` | `storage_oauth PASS (exit 0, 1s, 78 checks)` |
| task_ownership (79) | `/tmp/witq93-task_ownership` | 0 / 0 | `output/checks/task-ownership.json` `a502cb557fcd82f4…` — byte-identical | `task_ownership PASS (exit 0, 0s, 79 checks)` |
| credential_rotation (82) | `/tmp/witq90-credential_rotation` | 0 / 0 | `output/checks/credential_rotation.json` `1c6d73445479bb0f…` — byte-identical | `credential_rotation PASS (exit 0, 0s, 82 checks)` |
| media_upload (65) | `/tmp/witq92-media_upload` | 0 / 0 | `output/checks/media_upload.json` `4893bc38a0030e0b…` — byte-identical | `media_upload PASS (exit 0, 7s, 65 checks)` |
| drive_ownership (38) | `/tmp/witq89-drive_ownership` | 0 / 0 | `output/checks/drive-ownership.json` `5aee65e27b43d626…` — byte-identical | `drive_ownership PASS (exit 0, 0s, 38 checks)` |

Every room starts `compile_rc=0 errors=0 warnings=0` and `harness_build_rc=0`, and each of the five twins
also matches its leg on the **printed** channel (`cmp` of stdout against the leg's printed record), which
the report-file comparison alone would not have caught.

### What had to be found before that equality meant anything

`credential_rotation` reached its real checks only after five measured defects were removed, each one
found by instrumenting a scratch copy rather than by reading:

- **PKCE alphabet.** The seat computed the S256 challenge with the standard base64 alphabet:
  `proof=09ZRoQRgLMBgwNqKW0DYC+4TJJ5ns3PZgvBOcCHnOu4` against the peer's
  `expected=09ZRoQRgLMBgwNqKW0DYC-4TJJ5ns3PZgvBOcCHnOu4`. The leg uses
  `base64.urlsafe_b64encode(...).rstrip(b'=')`; the subject's notice moved from `"HTTP 400. Check server
  address, credentials, model and permissions."` to `"Connection authorization saved in encrypted app
  state; API-key credentials were retained"`.
- **Doubled state path.** `tokens()` indexed `state → drive → <section> → oauth_tokens`; the panic text
  carried the doubled key (`missing drive drive tokens`).
- **Override in the wrong handler.** The malformed-grant override lived in the peer's `refresh_token`
  grant branch, so the five `… malformed initial grant retains exact prior tokens` cases could never fail —
  the authorization-code exchange never saw a malformed grant. The leg monkeypatches the shared `Peer.issue`,
  which answers every grant route and registers nothing, so the override now lives in `issue()`.
- **Empty replacement token.** `PeerState::replacement_access` was declared, never assigned
  (`..PeerState::default()`), and used in the replacement body, so the peer answered `"access_token":""`
  and the subject marked the rotation uncertain — failing `null omission retains prior refresh compatibility`.
  The field is gone; the leg's literal `replacement-access` is used.
- **Model payload wrapped one level too deep.** The peer answered `/model/chat` with
  `{"chat":{"choices":[…]}}`; the leg selects the per-format value (`{…}[fmt]`), so the body is the bare
  protocol payload. Measured subject reaction to the wrapper: `notice="Missing choices"`,
  `notice_error=true`.

Two more defects were report-channel, not behaviour, and both were invisible to a report-file comparison:
`task_ownership` printed `79 task_ownership checks passed` where the leg prints its record plus a
`"count": 79` key (5922 vs 5907 bytes), and `storage_oauth` printed nothing where the leg prints a
*different* shape from what it writes (`dict(passed=names)`, 5201 bytes, against a written
`{passed: 78, checks: […]}`, 5217 bytes). Both twins now carry the leg's printed channel byte for byte.

### Audit disposition (every seat output read in full)

`storage_oauth` — the seat's single finding ("invents default port 80") is disproved by the instrument the
leg itself uses: `urlsplit('http://127.0.0.1/authorize').port` is `None` and
`http.client.HTTPConnection('127.0.0.1', None).port` is `80` (`HTTPConnection.default_port == 80`).
Behaviour kept, equivalence recorded in a comment; **accepted**.
`credential_rotation`, `task_ownership`, `drive_ownership` — **accepted** by the seats with full coverage
tables (82/79/38 rows), three mutations each, and pass-conversion censuses (32/33/9 rows) with nothing
invented. I re-ran the `drive_ownership` mutation premise: the leg literal really is
`'GET /share/second.txt '` with a trailing space, and the twin keeps it.
`media_upload` — the seat **rejected** on a "renamed check label" (`bounded 1` vs `bounded 0`).
Disproved from the leg's own byte target: its names are
`bounded 3, 5, 8, 10, 12, 14, 16, 18, 20, 61` — that check lives inside `exchange()` and never runs at
count 0 — and the twin's report is `cmp` byte-identical to that target, so no label differs. The seat's
premise about the leg's append order is wrong; the twin is **accepted**.

### Census

`VERIFICATION_PY_FILE_CEILING` 21 → **16** and `VERIFICATION_PY_LINE_CEILING` 2296 → **1813**, with the
planted fixtures moved in the same edit: `publish_refuses_verification_python_over_ceiling`
(51 files vs ceiling 16), `publish_refuses_verification_python_lines_over_ceiling`
(1814 vs 1813), `publish_passes_verification_python_lines_at_ceiling`,
`publish_python_census_byte_bound_is_two_sided`, `publish_passes_compliant_python_census`
(`verification files 16 lines 16 ceiling files 16 lines 1813`),
`publish_python_census_counts_move_when_a_file_is_added` and `project_python_census_counts_this_tree`
(`verification files 16 lines 1813 ceiling files 16 lines 1813;`). `verification/README.md` now runs
`output/verification/drive_ownership` and `output/verification/task_ownership`.

### The run log on the committed bytes (commit `c535ccb`, room `/tmp/room-c`)

A clean `git archive HEAD` extraction — no `output/`, no `.git`, `tooling/` symlinked from the host:

```
census files=16 lines=1813 twins=57
headless_rc=0
tools_rc=0                     twin helper tests: 9 cases
units_build_rc=0
gate_rc=0                      Rust cases passed: 878/878; isolated application cases: 57
                               output/checks/full-units.json
                               cb2fd261dc385e7ffb3db408e76a922aa702f17d985fecf0e02ca793923c467c
gate_units_rc=0                Selected cases: 310
verification_build_rc=0
harness_build_rc=0
output/harness storage_oauth task_ownership credential_rotation media_upload drive_ownership local_feedback
groups_rc=0                    summary: 6 pass, 0 fail, 0 unavailable of 71 groups
  storage_oauth      PASS (exit 0, 1s, 78 checks)
  task_ownership     PASS (exit 0, 0s, 79 checks)
  credential_rotation PASS (exit 0, 0s, 82 checks)
  media_upload       PASS (exit 0, 6s, 65 checks)
  drive_ownership    PASS (exit 0, 0s, 38 checks)
  local_feedback     PASS (exit 0, 0s, 29 checks)
```

The ratcheted census fixtures and `project_python_census_counts_this_tree` both pass on those bytes, so the
new 16 / 1813 ceilings are enforced by the same tool rows that measure the tree, and the delivered
`full-units.json` digest is unchanged from the previous batch.

## 2026-11-13 (evening) — `commands` and `query_feedback` go native, and the witness now compares the printed channel too

Two more owner-run legs retired in one batch (`0c508c4`, `db330c8`). The witness instrument itself changed:
`/tmp/twinwitness3.sh` is v2 plus `cmp w-twin.log w-py.log`, because five of the last six legs turned out to
print a *different record* from the one they write. A twin that only matches the file it writes is no longer
certified.

| Group | Room | Twin / leg rc | Canonical report (runner-derived) | Independent counts | Printed channel |
|---|---|---|---|---|---|
| commands | `/tmp/witq96-commands` | 0 / 0 | `output/checks/commands.json` `39bf8226…` byte-identical | `passed=86` | byte-identical to the leg's stdout |
| query_feedback | `/tmp/witq96-query_feedback` | 0 / 0 | `output/checks/query-feedback.json` `11a83f25…` byte-identical | `passed=74`, scope | byte-identical to the leg's stdout |

Runner rows on the witnessed bytes: `commands PASS (exit 0, 0s, 86 checks)`,
`query_feedback PASS (exit 0, 0s, 74 checks)`.

### The two `commands` pass-conversions the audit caught

The first audit of the grok-authored twin rejected it on two fabricated values, and both were real:

- A non-string tool reply became `String::new()`. The leg compares the tool reply against the evidence text
  directly, so a non-string reply fails there; the twin would have kept going. It now panics naming the check.
- `stream_protocol` defaulted to `"chat"` for a missing or non-string protocol. The leg is **not uniform**
  here — line 67 indexes `d['effect']['stream_protocol']` (a missing key is a KeyError) while line 69 writes
  `follow.get('stream_protocol') or 'chat'`. One helper could not be faithful to both, so the twin now has two:
  `effect_protocol` panics, `follow_protocol` keeps the default. A single "obviously right" default would have
  papered over leg line 67 forever.

The re-audit of the repaired bytes accepted: 86 checks present in leg order, both fixes cited against the leg
lines they mirror, and no remaining arm that can turn a leg-failing case into a twin pass.

`query_feedback` was accepted on its first audit: 28 leg `check` calls (74 executions) all present in order,
the leg's four bare `assert`s mirrored as twin panics, and the two arms that look permissive — `wire_text`
accepting an array wire, and the inquiry-id default — are each pre-empted by an earlier panic. Its report
shape is the inverted one again: the file carries `{passed: [74 names], scope}` and stdout carries the same
body plus `"count": 74`, so the twin builds two bodies.

### Census ratchet

`verification/` Python went 16 files / 1813 lines → 15 / 1741 (`commands.py`, 72 lines) → **14 files / 1682
lines** (`query_feedback.py`, 59 lines), 59 legs retired and 59 twins in `verification/*.rs`. Every
`tools/package.rs` ceiling fixture moved with it: the two-sided bound fixture
(`verification files 1 lines 1 ceiling files 14 lines 1682`), the compliant-census fixture
(`verification files 14 lines 14 ceiling files 14 lines 1682`), the over-ceiling refusals
(`files 51 exceed the recorded ceiling 14`, `lines 1683 exceed the recorded ceiling 1682`), and
`project_python_census_counts_this_tree` (`verification files 14 lines 1682 ceiling files 14 lines 1682;`).
`verification/README.md` now runs `output/verification/commands`.

### The run log on the committed bytes (commit `0c508c4`, room `/tmp/room-d`)

A clean `git archive HEAD` extraction — no `output/`, no `.git`, `tooling/` symlinked from the host:

```
census files=15 lines=1741 twins=58
headless_rc=0
tools_rc=0                     twin helper tests: 9 cases
units_build_rc=0
gate_rc=0                      Rust cases passed: 878/878; isolated application cases: 57
                               output/checks/full-units.json
                               cb2fd261dc385e7ffb3db408e76a922aa702f17d985fecf0e02ca793923c467c
gate_units_rc=0                Selected cases: 310
verification_build_rc=0
harness_build_rc=0
output/harness commands storage_oauth task_ownership credential_rotation media_upload drive_ownership local_feedback
groups_rc=0                    summary: 7 pass, 0 fail, 0 unavailable of 71 groups
  commands           PASS (exit 0, 0s, 86 checks)
  drive_ownership    PASS (exit 0, 0s, 38 checks)
  local_feedback     PASS (exit 0, 0s, 29 checks)
  task_ownership     PASS (exit 0, 0s, 79 checks)
  storage_oauth      PASS (exit 0, 1s, 78 checks)
  credential_rotation PASS (exit 0, 0s, 82 checks)
  media_upload       PASS (exit 0, 7s, 65 checks)
sweep_rc=0                     summary: 64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks
```

The 1682-line ceiling, the twelve moved census fixtures and `project_python_census_counts_this_tree` all
measure the same tree the publisher publishes, and the delivered `full-units.json` digest is unchanged again.
The seven `unavailable` groups are the device-dependent ones (installed Android surfaces); none is a native
twin regression — no group reported `fail`, and the runner reported `0 absent`.

### Two more in the same batch: `diagnostics` (31) and `workspace_navigation` (83) — commit `e768551`

| Group | Room | Twin / leg rc | Canonical report | Printed channel | Runner row |
|---|---|---|---|---|---|
| diagnostics | `/tmp/witq99-diagnostics` | 0 / 0 | `output/checks/diagnostics.json` `14860aae…` byte-identical | byte-identical | `diagnostics PASS (exit 0, 0s, 31 checks)` |
| workspace_navigation | `/tmp/witq98-workspace_navigation` | 0 / 0 | `output/checks/workspace-navigation.json` `0e198af6…` byte-identical | byte-identical (leg adds `count: 83`) | `workspace_navigation PASS (exit 0, 0s, 83 checks)` |

The diagnostics audit **accepted** the twin and still caught something: its coverage row for leg line 52
recorded the predicate as `result.to_string().contains("input.txt")`, where the leg evaluates
`'input.txt' in result` on the reply's own string value. That is the same class as the `commands` finding
(a non-string tool reply quietly becoming a string), so it was not waved through: the twin now requires
`Value::Str` and panics naming the check otherwise, and the repaired bytes were rebuilt, re-run against both
byte targets, re-witnessed (`witq99`) and re-audited (accept). An accepting audit is not a clean bill of
health — its coverage table is evidence to read line by line.

The `diagnostics` writer draft needed five signature repairs before it even compiled: a `fields` **array**
indexed as an object (the leg uses `next(f['value'] for f in … if f['key']=='runs')`), an `Option<&Value>`
vs `&Value` `matches!` arm, a missing second `get()` argument, two `.and_then(Value::str_)` fn-pointer arms,
and `scratch_root()` used as a `Path`. `workspace_navigation` (83 checks, real loopback MCP peers plus the
Android document callbacks) came out of its lane compiling and byte-exact on the first try.

Census: 12 files / 1517 lines left in `verification/` (61 legs retired), ceilings and all fixtures moved.

### The run log on the committed bytes (commit `e768551`, room `/tmp/room-e`)

A clean `git archive HEAD` extraction — no `output/`, no `.git`, `tooling/` symlinked from the host:

```
census files=12 lines=1517 twins=61
headless_rc=0
tools_rc=0                     twin helper tests: 9 cases
units_build_rc=0
gate_rc=0                      Rust cases passed: 878/878; isolated application cases: 57
                               output/checks/full-units.json
                               cb2fd261dc385e7ffb3db408e76a922aa702f17d985fecf0e02ca793923c467c
gate_units_rc=0                Selected cases: 310
verification_build_rc=0
harness_build_rc=0
output/harness commands diagnostics workspace_navigation query_feedback storage_oauth task_ownership
               credential_rotation media_upload drive_ownership local_feedback
groups_rc=0                    summary: 10 pass, 0 fail, 0 unavailable of 71 groups
  commands            PASS (exit 0, 0s, 86 checks)
  diagnostics         PASS (exit 0, 0s, 31 checks)
  workspace_navigation PASS (exit 0, 0s, 83 checks)
  query_feedback      PASS (exit 0, 0s, 74 checks)
  storage_oauth       PASS (exit 0, 1s, 78 checks)
  task_ownership      PASS (exit 0, 0s, 79 checks)
  credential_rotation PASS (exit 0, 0s, 82 checks)
  media_upload        PASS (exit 0, 6s, 65 checks)
  drive_ownership     PASS (exit 0, 0s, 38 checks)
  local_feedback      PASS (exit 0, 0s, 29 checks)
sweep_rc=0                     summary: 64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks
```

The census line is the publisher's own count of the tree it publishes: 12 Python files, 1517 lines, 61 native
twins. The delivered `full-units.json` digest is byte-identical to the previous two rooms' (`cb2fd261…`), so
the three commits in this batch moved no native product bytes — only verification legs and their ceilings.
The runner's pre-run `recorded in verification/RESULTS.json: 71 groups, 4248 checks` line is a read of the
stale manifest, not this sweep's verdict; the sweep's own row is the last line, and the seven `unavailable`
groups are the device-dependent installed-Android surfaces, unchanged from room `d`.

## 2026-11-13 (late evening) — `mcp_images` and `network_drive` go native, and a draft that emitted 57 names for 46 checks is caught before it can lie

| Group | Room | Twin / leg rc | Canonical report | Printed channel | Runner row |
|---|---|---|---|---|---|
| mcp_images | `/tmp/witq102-mcp_images` | 0 / 0 | `output/checks/mcp-images.json` `9af2b833…` byte-identical | byte-identical (`{passed:42, checks:[42 names], requests:12}` compact) | `mcp_images PASS (exit 0, 0s, 42 checks)` |
| network_drive | `/tmp/witq103-network_drive` | 0 / 0 | `output/checks/network-results.json` `9b61a7a2…` (2895 B) byte-identical | byte-identical (`46 connected WebDAV checks passed`, 34 B) | `network_drive PASS (exit 0, 0s, 46 checks)` |

`mcp_images` (172 Python lines, 42 checks, real loopback MCP peer, PNG/JPEG byte provenance, native preview
readback and export) came out of its earlier lane already byte-exact on both channels; the witness only had to
prove it in a clean room, and the audit accepted it with a full coverage table and no pass-conversion arm.

`network_drive` (117 Python lines, 46 checks) is the opposite case: the writer seat's draft compiled and
*printed*, but it was not a twin of the leg, and nothing short of reading it line by line would have shown
that. Four separate defects, all found by reading the draft against the leg and then letting the twin run:

- the push arithmetic was wrong. The leg appends **two** names per create label
  (`manual create and actual readback comparison: <label>` then
  `completed create rejects duplicate verification: <bytes>`); the draft appended three per label starting at
  index 30, and pushed names at intermediate assertion sites elsewhere, so it emitted **57** names for a
  46-name leg and reused names for the wrong assertions. Intermediate assertions are now `keep_assert`
  (assert, don't name) and only leg-append sites push.
- the duplicate-verification **step** was missing: the leg replays the readback reply against the acknowledged
  effect (`reply(dict(state=verified['state'], effect=acknowledged['effect']), content, 200)`) and asserts the
  state is unchanged with no new effect. Without that call, half the names in that block had no evidence.
- the event `root` was the project root, not the leg's scratch directory, so the subject resolved its
  file surface differently from the leg. The scratch path is now threaded through every `event`/`reply` call.
- the draft cloned `st` for the agent-tools change instead of mutating it, so the last child block ran with
  `tools='delegate,drive_list'` and produced `effect: null` where the leg asserts
  `effect['purpose'] == 'model'`. The twin panicked on it (`missing purpose in null`) — a failure that shows
  as a failure — and the fix was to mutate the same state the leg mutates.

Two semantic arms were also tightened against the rules that cost earlier twins reds: `not refused['state'].get('remote')`
is now `is_none_or(|rows| !twin::truthy(rows))` (absent, null, empty list or empty string all satisfy the leg),
and `verified['state'].get('drive_creation') is None` accepts absent **or** present-null, and `limit >= len+256`
compares the parsed number, not two `Value`s.

Census: 10 files / 1228 lines left in `verification/` (63 legs retired); the two ceilings and all census
fixtures in `tools/package.rs` moved to 10 / 1228.

### The run log on the committed bytes (commit `569a28e`, room `/tmp/room-f`)

```
census files=10 lines=1228 twins=63
bash headless.sh                   headless_rc=0
bash tools/build-units.sh          units_build_rc=0
bash tools/build-tools.sh          tools_rc=0
output/tools/gate output/units     gate_rc=0    Rust cases passed: 878/878; isolated application cases: 57
                                                 cb2fd261dc385e7ffb3db408e76a922aa702f17d985fecf0e02ca793923c467c  output/checks/full-units.json
output/tools/gate --units          gate_units_rc=0    Selected cases: 310
bash tools/build-verification.sh   verification_build_rc=0    63 twins
bash tools/build-harness.sh        harness_build_rc=0
output/harness network_drive drive_ownership commands local_feedback query_feedback
               workspace_navigation mcp_images storage_oauth credential_rotation diagnostics
groups_rc=0                    summary: 10 pass, 0 fail, 0 unavailable of 71 groups
  network_drive        PASS (exit 0, 0s, 46 checks)
  drive_ownership      PASS (exit 0, 0s, 38 checks)
  commands             PASS (exit 0, 0s, 86 checks)
  local_feedback       PASS (exit 0, 0s, 29 checks)
  query_feedback       PASS (exit 0, 0s, 74 checks)
  workspace_navigation PASS (exit 0, 0s, 83 checks)
  mcp_images           PASS (exit 0, 0s, 42 checks)
  storage_oauth        PASS (exit 0, 1s, 78 checks)
  credential_rotation  PASS (exit 0, 0s, 82 checks)
  diagnostics          PASS (exit 0, 0s, 31 checks)
output/harness sweep               sweep_rc=0    summary: 64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks
```

The two groups that went native in `569a28e` — `mcp_images` (42) and `network_drive` (46) — were both served by
native twins from `output/verification/` in this room, and the sweep row is unchanged from rooms `d` and `e`
(`64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks`) with the same delivered
`full-units.json` digest `cb2fd261…`. That is what retiring a leg must look like: identical group rows and
identical product bytes, only now the group is a binary.

## 2026-11-13 (night) — three more legs go native, and an audit that was half right still stops a twin

| Group | Room | Twin / leg rc | Canonical report | Printed channel | Runner row |
|---|---|---|---|---|---|
| product_flow | `/tmp/witq106-product_flow` | 0 / 0 | `output/checks/product-flow.json` (3273 B) byte-identical, twin sha `d426e259…` | byte-identical (3288 B) | `product_flow PASS` in the same room |
| prompt_reference | `/tmp/wit-prompt_reference` (witq107, after repair) | 0 / 0 | `output/checks/prompt-reference.json` byte-identical | byte-identical (3837 B) | `prompt_reference PASS (exit 0, 0s, 52 checks)` |
| pe_declarations | `/tmp/witq108-pe_declarations` | 0 / 0 | `output/checks/pe-declarations.json` `ac3759bd…` (3800 B) byte-identical | byte-identical (3815 B) | `pe_declarations PASS (exit 0, 2s, 43 checks)` |

`prompt_reference` is the case worth writing down, because the audit and the witness disagreed and both were
right about different things.

- The first audit **rejected** the draft on two counts: it printed a hardcoded-looking `52` and it "collected
  only 48 checks"; and it fabricated `request_bytes` as an empty array while omitting the leg's
  `if args.previous_controller:` branch. The count claim was **contradicted by the witness** — the leg run the
  way `tools/harness.rs` runs it here (no `output/previous-controller` file, so no flag) emits **52** names in
  both channels, `request_bytes: []`, and the twin was byte-identical to it. The second claim was **correct and
  fatal**: the twin accepted no `--previous-controller` at all, so the runner's documented invocation
  (`verification/README.md`, and `sweep_arguments` at `tools/harness.rs:1078`, which passes the flag exactly
  when that controller file exists) would have had the twin report an empty comparison it never performed.
  "A leg-failing case can pass the twin" is the standard that decides, not the seat's arithmetic.
- The repair implements the branch as the leg has it: spawn the prior controller at the same scratch root, send
  the same draft per protocol, record `{protocol, previous_request_bytes, current_request_bytes}` from the two
  actual request wires, assert `current < previous * 0.7`, then cancel — in the leg's slot, between the fifth and
  sixth check of each protocol, so the name order is the leg's. The expected-count self-check became 52/56
  instead of a bare 52.
- The branch was then **made red on purpose**, on both instruments, with the current binary as the "prior"
  controller: leg `rc=1 AssertionError: chat actual read-only request is substantially smaller`, twin
  `rc=101 panicked … chat actual read-only request is substantially smaller`. A threshold mutant (`0.7 → 1.7`,
  scratch copy only, then deleted) turns green with 56 names and measured rows — `chat 4718/4718`,
  `local 4692/4692`, `messages 4699/4699`, `content 4678/4678` — so the numbers are the controller's own wire
  lengths, not constants. Re-witnessed byte-identical, then re-audited: **accept**, 56/56 coverage, and the seat
  independently confirmed the count guard is twin-stronger rather than a fabrication.

`pe_declarations` (132 Python lines, 43 checks) is the heaviest twin in the tree: it drives the real NDK
toolchain — `clang`, `lld-link`, `llvm-readobj` — to build and re-poison an original assembled/linked PE/COFF
fixture with DIR64 base relocations, then runs typed declaration, callback, export/readback, clean-room
implement/verify/refusal, and saved-capture (`.har`/`.saz`, member/section/file-size caps) rows against it. The
first draft of this leg (a MiniMax seat from an earlier round) was a 152-line stub whose `sha256_hex` returned
`"00000000…"`; it was thrown out rather than repaired, and the lane's 1813-line port replaced it. The audit
accepted it with a 43-row coverage table and no pass-conversion arm.

`product_flow` (147 Python lines, 37 checks) came out of its lane byte-exact on both channels and was accepted
with mutation arms of its own.

Census: **7 files / 911 lines** left in `verification/` (66 legs retired); the two ceilings and all twelve census
fixtures in `tools/package.rs` moved to 7 / 911, and `verification/README.md` now names
`output/verification/pe_declarations` and `output/verification/prompt_reference`.

## 2026-11-13 (late night) — the largest reachable leg goes native, and a clean-room row that was the instrument's fault

### The run log on the committed bytes (commit `6e124f3`, room `/tmp/room-g`)

A `git archive HEAD` room with no `output/` and no `.git`, measured with the sanctioned instruments only.

| Instrument | Result |
|---|---|
| census in the room | 7 Python legs / 911 lines, 66 twins compiled |
| `bash headless.sh` | rc 0 |
| `bash tools/build-tools.sh output/tools` | rc 0; executed == declared per tool: alignment 2, readback 24, checkpoint 5, signature 5, apk 6, toolchain 10, gate 18, package 46, harness 28; twin helper tests 9 |
| `bash tools/build-units.sh output/units` | rc 0 |
| `output/tools/gate output/units` | `Rust cases passed: 878/878; isolated application cases: 57` |
| `output/tools/gate --units output/units` | `Selected cases: 310` |
| `output/checks/full-units.json` | `cb2fd261dc385e7ffb3db408e76a922aa702f17d985fecf0e02ca793923c467c` — identical to the previous room, so the three retired legs moved no core byte |
| `output/harness sweep` | `64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4146 checks` |

The targeted row initially read **9 pass / 0 fail / 1 unavailable**, and the unavailable row was
`product_flow UNAVAILABLE (not executed, missing: path output/published-controller reference build (the group
opens this path itself))`. That is the room script's gap, not the product's: `rc-g.sh` never seeds
`output/published-controller`, which `product_flow` opens by itself. Copying the reference build into the room
and re-running the same ten groups gives **10 pass / 0 fail / 0 unavailable** (`product_flow PASS (exit 0, 0s,
37 checks)`). Room scripts from here on seed that path before the targeted row.

### hosted_phases: 370 Python lines, 118 checks

`hosted_phases.py` was the largest leg still reachable on this machine. The lane's twin
(`verification/hosted_phases.rs`, 2153 lines, sha `ebbe53a7…`) was witnessed in `/tmp/witq109-hosted_phases`
(one `git archive HEAD` of `6e124f3`, twin copied in, python leg left in place): twin and leg both exit 0,
`output/checks/hosted-phases.json` 8574 B sha `34ec526d3dfd…` on both instruments with **CMP-FILE
byte-identical**, stdout 8713 B with **CMP-STDOUT byte-identical**, independent counts `passed=118`, and in the
same room the runner row `hosted_phases PASS (exit 0, 1s, 118 checks)`.

The audit (`/tmp/mm-audit-hosted_phases.out`, read in full) **accepted** it: all 118 leg checks mapped to twin
predicates in leg order with nothing absent, merged, renamed or reordered; a real twin-red/leg-green mutation
(twin line 121 `num_eq(…, "passed", 1)` → `0`, which the leg's own `Partial report REQUIRED-ALPHA.` input at
leg lines 33–34 makes differ); a report-byte mutation on the scope string at twin line 7; an inert control that
must stay green; and a 24-row pass-conversion census of the twin's helper arms, every one shown unable to turn
a leg-failing case into a twin pass.

The retirement itself was caught by the instrument, not by me: after the ceilings moved, the first
`tools/build-tools.sh` run reported **45 passed / 1 failed**, `publish_python_census_byte_bound_is_two_sided`
panicking at `package.rs:4098` because that fixture still pinned `ceiling files 7 lines 911` in the census
sentence. Ten census sites had to move, not nine. With the tenth moved the package suite is 46/46 and the whole
tool suite runs with executed == declared counts.

Census: **6 files / 541 lines** left (67 legs retired): `binary_flow.py` 271, `turn_background.py` 75,
`live_web.py` 71, `apk_alignment.py` 54, `history_migration.py` 43, and the shared
`transport.py` 27 — which now has **zero importers**: `verification/*.py` no longer imports it, its protocol is
native in `core/media_upload.rs` and `core/media_stream.rs`, and the only thing that keeps it off the runner's
"undeclared script" list is `const HELPERS: &[&str] = &["transport"];` at `tools/harness.rs:35`. It goes next.

New ground opened the same night: an **unsigned Lab APK** (`CLIENT_UNSIGNED=1 CLIENT_VERSION_CODE=1794569618
bash package.sh` → `output/2026-11-13__11-33-43Z/RLLM-unsigned.apk`, build rc 0) makes `apk_alignment` (11
checks) and `turn_background` (28 checks) runnable through `RLLM_CHECK_APK` without a keystore. Both twins
compile; both first runs went red for their own reasons — `apk_alignment` parsed the whole publication manifest
instead of the leg's `['signed_apk']` record, then hit the shared JSON parser's 8 MiB cap while round-tripping
the whole APK member inventory as hex through `python3`; `turn_background` panicked with
`archive listing was not a JSON array` at line 434. Both are shelling out to `python3` for ZIP work, which is
itself an R-rule smell inside a Rust twin, so the repair being driven now is a real pure-std ZIP
reader/writer (`zip_names`, `zip_read`, `zip_entries_of`, `zip_write`, `crc32`, with DEFLATE inflate) in
`verification/twin/mod.rs`, and both twins off `python3` onto it.

## 2026-11-13 (night shift two) — transport.py is gone, four more legs have native twins, and the ZIP reading left python

The python census moved from **6 files / 541 lines to 5 files / 514 lines**: `verification/transport.py` (27 lines)
is deleted and had zero importers anywhere in `verification/`, `tools/` or `core/`. `tools/package.rs` carries the new
ceilings at all ten census sites and `tools/harness.rs` now has `const HELPERS: &[&str] = &[];`, so no group is declared
through a helper leg. The move first went red — `output/tools/package-tests` panicked with
`python_census: ... verification files 1 lines 1 ceiling files 7 lines 911` because the byte-bound refusal fixture still
carried the old census sentence; that is the fixture catching a missed census site, and after the tenth site the suite is
`46` cases green. Gate on these bytes: `bash tools/build-tools.sh output/tools` → `TOOLS-RC=0` (it ends by calling
`tools/build-verification.sh`), twin helper tests `17` executed == `17` declared, and every native twin compiling together.

`verification/twin/mod.rs` grew a pure-standard-library ZIP layer — `crc32`, zlib/deflate inflate, a STORED+DEFLATE
reader (`catalog`/`read_member`/`zip_names`/`zip_entries_of`) and a STORED writer (`zip_write`). It was proved against
`python zipfile` on the real 25-member unsigned Lab APK: per-member names, lengths and sha256 digests identical. The first
audit of that layer rejected for a pass-conversion gap — a member whose *local* header lied about method, crc or sizes was
read anyway — so `read_member` now rejects any local header that disagrees with the central directory. Its re-audit
rejected twice more, and both findings were real: `zip_read` returned the *first* member of a duplicated name where
`python zipfile` returns the *last*, and `catalog` silently re-derived every absolute offset from the footer instead of
trusting the archive's own central-directory extent. `zip_read` now takes the last match and `catalog` refuses a
directory that does not end at its own end record, with tests for duplicate resolution, the shifted-preamble case and a
member extent that runs past the end of the file. Helper tests went 15 → 17 → 19 (executed == declared) and the APK's
per-member digests stayed identical (`/tmp/zprobe-twin.txt` == `/tmp/zprobe-py.txt`, aggregate `b4e7616b1882…`).

Four legs now have twins that were run against their legs:

| group | result on the same bytes |
|---|---|
| `turn_background` | 28 checks, clean-room witness byte-identical on both channels (`269f3240961f…`, stdout identical), audit accept. The dex class-descriptor parse needed the real dexdump form — `: 'Lcom/local/client/TurnService;'` — so the terminator is `;'`, not `';`, and the leading `L` is stripped. |
| `apk_alignment` | 11 checks, clean-room room with `TOOLS=1`: reports byte-identical (`36ff8454aa66…`) and the room's own row printed `apk_alignment PASS (exit 0, 4s, 11 checks)`. The twin's stdout is 1222 B against the leg's 1380 B and the entire difference is two `UserWarning: Duplicate name: AndroidManifest.xml` lines python's own zipfile emits inside the leg. Audit accept. |
| `live_web` | both instruments run with `--live`: leg rc=0, twin rc=0, identical nine names in identical order, identical scope. The observation carries the identical URL and identical byte length 44629 but a different sha256, because the upstream body is not fetch-stable — three repeats of the leg's own curl gave `269630dcfd18…`, `2b81f57a07bd…`, `2b81f57a07bd…` — so byte-identical transport observations are unattainable for this leg. Audit accepted only after two real fixes: the agent settings must carry `subagents` as an index **list** and `model` as **string** indices (otherwise the controller publishes `children:[]` and the delegation is blocked before start), and the model payload text must use `json.dumps` default separators. |
| `history_migration` | matched red: leg exits 1 with a traceback at `verification/history_migration.py:18`, twin exits 101 with `prior controller did not request approval` at `verification/history_migration.rs:141` — the same step, and neither writes a report. Revisions 1793395702/1793592809 are unrecoverable, so this leg cannot be made green by any reachable input. Audit accept. |

`binary_flow` is still red, and honestly so: with `output/published-controller` substituted for the missing
`output/pre-branch-controller`, the leg reaches `actual previous controller retains its original decoder meaning` while
the twin stopped earlier at `review creates owned shared-engine worker`. That gap was a twin divergence, not a comparator
artefact, and it is now closed: the twin reaches the same comparator step and fails there, so the leg's unreachable
comparator is shown as a failure rather than hidden behind an earlier twin-only stop.

### The ZIP layer's third repair, and what a full mutation battery found

A third audit reject was also real: a DEFLATE member was accepted when the inflater stopped **short** of the declared
`compressed_size`, so an archive could pass on an inflated prefix with unread bytes still inside the member's extent. The
reader now takes `(plain, consumed)` from `inflate_deflate_consumed` and refuses any member whose consumed count differs
from the declared extent.

The fourth audit pass confirmed every claim it was asked about (M1–M11) and rejected on two points, and both were settled
by measurement rather than argument:
- it asserted that unsupported general-purpose flag semantics (a data descriptor) "may be accepted"; compiled against
  these bytes, such a member is refused — `PROBE REJECTED: corrupt ZIP member payload: local header disagrees with the
  central directory` — because a data descriptor zeroes the local CRC and sizes and the directory cross-check rejects it;
- it found that the encrypted-member guard had **no test at all**: mutating `if member.flags & 0x0041 != 0` to
  `if false` left the suite green at 21 cases. That is a coverage hole, so two tests were added —
  `encrypted_members_are_rejected_in_both_headers` (the fixture sets the bit in the local **and** central header, so a
  reader that trusts only the directory cannot read ciphertext) and `data_descriptor_member_is_rejected`, pinning the
  measured rejection above. Helper tests are now **23 executed == 23 declared**.

Then the module was put through a measured battery, each mutant one line and each restore byte-identical
(`RESTORED-IDENTICAL` after every step, final sha256 `ce237199b54c…`, baseline `23 passed; 0 failed`, 0 warnings):

| mutant | named test that went red |
|---|---|
| duplicate-name lookup loses `.rev()` | `stored_zip_round_trips_empty_and_compressible_members` |
| DEFLATE consumption guard → `if false` | `deflate_extent_with_bytes_after_the_stream_is_rejected` |
| flag guard → `if false` | `encrypted_members_are_rejected_in_both_headers` (the data-descriptor test stayed green — the cross-check refuses that archive independently) |
| local-vs-directory guard → `if false && …` | `local_header_disagreeing_with_the_directory_is_rejected` |
| central-directory extent → `if false && …` | `a_preamble_that_shifts_the_directory_is_rejected` |
| **inert control**: writer's version field 20 → 45 | none — suite stayed green at 23 |
| **over-tight control**: flag guard unconditional | 10 tests — the suite also covers the honest read path |

The APK byte-exactness was re-measured after every change: the Rust probe recompiled against the final module bytes and
`cmp` reported the 25-member tables identical to `python zipfile`'s (aggregate `b4e7616b1882…`).

### Fifth pass: one more real fail-open, plus branch-level honesty

The fifth audit pass confirmed M1–M13 and then found something the earlier passes had missed: the encryption check reads the
**central directory's** flags, so an archive that sets the encryption bit only in the **local** header was read as plaintext.
That is a genuine fail-open, and it is now closed by comparing the general-purpose flag word local-vs-directory
(`let local_flags = u16_le(bytes, offset + 6); if local_flags != member.flags || …`), with
`local_flag_drift_is_rejected` pinning it. The same pass listed guards with no test at all, so
`local_name_drift_is_rejected` and `local_compressed_size_drift_is_rejected` were added — helper tests 23 → **26** — and a
second battery was run one mutant at a time, every restore byte-identical:

| mutant | result |
|---|---|
| CRC comparison → `if false && …` (the seat's own proposal) | `corrupt_member_fails_the_crc_check` red |
| local **method** branch removed | `local_header_disagreeing_with_the_directory_is_rejected` red |
| local **compressed-size** branch removed | suite still green — the same `||` disjunction still refuses through the other size branch |
| **both** size branches removed | `local_compressed_size_drift_is_rejected` red (so the pair is collectively live, individually indistinguishable) |
| local-name comparison removed | `local_name_drift_is_rejected` red |
| local-flags branch removed | `local_flag_drift_is_rejected` red |
| duplicate lookup / DEFLATE consumption / encrypted guard / directory extent | their named test red, one each |
| inert control (writer version field) | all 26 green |
| over-tight control (flag guard unconditional) | 13 red |

The dex class-descriptor parse was tightened for the one divergence that pass named: the leg's character class `[^';]+`
never accepts an apostrophe inside a descriptor, so the twin now refuses one instead of accepting `Turn'X` as a class name.
`verification/turn_background.rs` also lost its two compiler warnings, so its room compiles at 0 warnings.

Both twins were then **re-witnessed against the final module bytes**, because the module had changed since their first
witnesses: `apk_alignment` (room `/tmp/witq2-apk_alignment`, `TOOLS=1`) compiled 0 errors/0 warnings and wrote a report
byte-identical to the leg's (`36ff8454aa66…`), with the room's own harness row `apk_alignment PASS (exit 0, 5s, 11 checks)`;
`turn_background` (room `/tmp/witq2-turn_background`) compiled **0 warnings** and was byte-identical on both channels
(`269f3240961f…` report, stdout identical), row `turn_background PASS (exit 0, 0s, 28 checks)`. Gate on the final module:
`GATE17-RC=0`, package suite 46 cases, `twin helper tests … cases: 26` with 26 declared.

### Two python dependencies with different status

A census of `Command::new("python…")` inside native code found three sites, and they are not the same case:

| site | what it spawns | adjudication |
|---|---|---|
| `tools/harness.rs:1120` | `verification/<group>.py` | The runner running a python **leg** — the leg is python by definition, not a violation. |
| `verification/binary_flow.rs:2455` | `python3 -c 'import zipfile…'` | **A real violation**, fixed: `read_zip_json` now uses `twin::zip_read` + `twin::Value::parse`. Measured with two synthetic baselines (one DEFLATE member, one STORED): the parsed manifest re-serialises to 302 bytes with sha256 `2778cd7c20be…` in both instruments, and the group still fails at the same check as its leg (`COMPILE rc=0 errors=0 warnings=0`, `TWIN rc=101` / `LEG rc=1`, both `actual previous controller retains its original decoder meaning`). The `--auth-baseline` branch is opt-in (`RLLM_AUTH_BASELINE`) and unset in the current sweep, so the probe rather than the group row is what exercises it. |
| `verification/snapshot_identity.rs:436` | a 40-line `ssl`-based TLS fixture server | **Kept as python, deliberately.** That group already has no `.py` leg; the fixture exists only to be a TLS peer for the *production Android TLS block* running on the host JVM, and nothing in `core/` implements a TLS record layer (census: no `ClientHello`/`ServerHello`/`ChangeCipherSpec` anywhere), so a pure-std replacement would mean writing a TLS 1.2/1.3 server (RSA, AES-GCM, PRF, certificate chain) into the verification instrument — the `compatibility only` clause of the R-rule, not instrumentation written in the wrong language. Two delegated attempts to build exactly that both timed out without writing a byte, which is the size of the job, not an oversight. |

### The eighth pass accepted the module and named its own remaining gaps

The eighth audit of `verification/twin/mod.rs` accepted the bytes at sha256 `49cdb01a423ced…` with M1–M21 confirmed: the real-archive census (34 archives, every member table byte-identical to `python zipfile`), the `read_zip_json` transcripts at `cmp=IDENTICAL` for both DEFLATE and STORED, the matched red pair (`LEG rc=1` / `TWIN rc=101`, both naming `actual previous controller retains its original decoder meaning`, `python processes started by the twin = 0`), and the branch-level battery. It still listed guards it believed no fixture reaches — the local-header `PK\x03\x04` signature, the two EOCD disk fields, the unequal-total-count clause, the ZIP64 end-record and member sentinels, the unsupported-method arm, the "end of central directory not found" branch, and the reverse-direction name comparison — and it proposed the local-signature mutation itself.

The test-quality seat that had rejected the same file on one finding ("No printed mutation turns either JSON test red") accepted the closure: `ZM-JSON-A` (duplicate-field guard → `if false && …`) turned `a_duplicate_object_field_panics_where_python_keeps_the_last` red, `ZM-JSON-B2` (a `Some(b'N' | b'I')` arm before the catch-all) turned `non_standard_float_literals_that_the_leg_accepts_are_refused` red, and the earlier `ZM-JSON-B` leg — still green — is kept as the measurement that `NaN`/`Infinity` never reach the lowercase-`n` arm at all. That pass named one more gap: the JSON nesting limit.

### The increment that closes the named guard list

Ten fixtures, one per named guard, each patching only the field that reaches its own guard:

| fixture | field it patches | mutant | named reds |
|---|---|---|---|
| `an_archive_starting_on_another_disk_is_refused` | `eocd+4` (disk number) | `ZM-T2` — drop the `disk_cd` disjunct | that test only |
| `a_central_directory_on_another_disk_is_refused` | `eocd+6` (directory disk) | `ZM-T1` — drop the `disk` disjunct | that test only |
| `disagreeing_total_counts_are_refused_as_multi_disk` | `eocd+8` alone (not `+10`) | `ZM-T3` | that test only |
| `a_zip64_end_record_sentinel_is_refused` | `eocd+12..16` = `0xffffffff` | `ZM-T4` | that test only |
| `a_zip64_directory_member_sentinel_is_refused` | `directory+20..24` = `0xffffffff` | `ZM-T5` | that test only |
| `a_local_header_with_the_wrong_signature_is_refused` | `raw[0] = 0` | `ZM-T6` — the seat's own proposed mutation, run verbatim | that test only |
| `a_method_outside_the_reader_is_refused_in_both_headers` | method field in **both** headers = 12 | `ZM-T7` | that test only |
| `a_directory_name_drift_is_refused_against_the_local_header` | `directory+46` = `X` | `ZM-T8` — drop the local-name comparison | this test **and** `local_name_drift_is_rejected` |
| `an_archive_without_an_end_record_is_refused` | end-record signature byte = 0 | `ZM-T9` — scan returns a guessed offset | this test **and** `truncated_archive_is_rejected` |
| `nesting_past_the_parser_limit_is_refused_where_the_leg_parses` | — | `ZM-T10` — limit 128 → 128 000 | that test only |

Two rows are reported as two-red mutants rather than one: `ZM-T8` proves both name directions share a single guard, and `ZM-T9` shows the same EOCD scan is what makes the truncation refusal land. The inert control (`ZM-CONTROL`, scan bound 65535 → 65536) left all 44 green, and every restore was byte-identical at `a2e42166bca3…` (`BASE-SHA` == `ALL-RESTORED`), with the baseline at `44 passed; 0 failed` and 0 warnings.

The nesting boundary was measured rather than assumed: the parser reads 129 levels and refuses 130 with `invalid JSON (JSON nesting limit exceeded)`, while the leg's `json.loads` reads 500 — the same fail-closed direction as the duplicate-key and non-standard-float pins, with the test pinning both sides of the boundary so raising the limit is a red test rather than a silent widening.

Helper tests went **34 → 44 declared and 44 executed**. The increment is test-only: every line before the file's first `#[cfg(test)]` is byte-identical between the accepted bytes and these, so nothing the eighth pass confirmed about reader behaviour could have moved — and the gate and both witnesses were re-run on these bytes anyway.

### The binary_flow twin's own tests, one real fail-open, and the leg that retires

The `verification/binary_flow.rs` audit accepted the twin but qualified it: C2 (the new tests had not been mutation-tested), C7 (`parse_int` diverged from the leg), C8 (the census retirement was not done, so the commit was not delivery-complete). C7 was a **real fail-open**, found while adjudicating and not before: the leg converts with python `int(v, 0)`, which raises on `int("09", 0)`, while `"09".parse::<i64>()` returns `Ok(9)` — the twin was reading a value the leg refuses. It now refuses a decimal token with a leading zero unless every digit is zero, sign-aware, mirroring the leg's base-0 rule.

Six tests pin the twin's own parsers, and each was then turned red by a named mutant:

| test | what it pins | mutant that turned it red |
|---|---|---|
| `a_decimal_with_a_leading_zero_is_refused_where_python_raises` | `09`/`-09`/`010` → none, `0`/`00`/`10` → value | `BF-A` — guard replaced with `true` (2 warnings, both the now-unused `digits`) |
| `only_the_forms_the_legs_regex_can_capture_are_read` | `0x20`/`0X20` read, `0o17`/`0b101`/`1_0` refused | `BF-B` — accept a `0o` prefix the leg's regex cannot capture |
| `a_pt_load_block_without_readable_labels_is_not_invented` | a second `PT_LOAD` with `Offset: n/a` yields **one** row | `BF-C` — pad the half-read row with zeros |
| `an_image_base_field_without_a_number_is_none_not_zero` | an absent `ImageBase` stays `None` | `BF-D` — `.or(Some(0))` |
| `the_pe_call_pair_comes_from_the_first_bl_line` | the pair comes from the first `bl 0x…` | `BF-E` — scan lines in reverse |
| `a_non_utf8_archive_member_is_refused_naming_the_member` | a UTF-16 member panics naming `manifest` | `BF-F` — `String::from_utf8_lossy` instead of refusing |

Battery baseline `50 passed; 0 failed`, 0 errors, 0 warnings; `BF-CONTROL` (an inert hex leading-zero trim) left all 50 green; `BASE-SHA` == `ALL-RESTORED` == `8f8e208ccfe0…`.

Then the retirement the leg's audit asked for: `verification/binary_flow.py` (271 lines) is deleted and the group runs as the native twin. The runner already routes it — `tools/harness.rs` `group_command` executes `output/verification/<name>` when that binary exists and falls back to `verification/<name>.py` only otherwise — and the pair log on these bytes shows the twin starting **0** python processes while still reaching the leg's own check (`LEG rc=1` / `TWIN rc=101`, both `actual previous controller retains its original decoder meaning`). The group stays red in the sweep for the missing comparator revisions; retiring the leg did not change that verdict, it removed the interpreter that produced it.

Census moved with the deletion, measured rather than asserted: `verification/` Python went **5 files / 514 lines → 4 files / 243 lines** (`apk_alignment.py` 54, `turn_background.py` 75, `live_web.py` 71, `history_migration.py` 43), tracked-tree **6/629 → 5/358**, and `tools/package.rs` ceilings moved `5/514 → 4/243` with every refusal sentence and the real-tree census test (`verification files 4 lines 243 ceiling files 4 lines 243`) re-pinned to the new numbers. `verification/README.md` now names the native command for that group, and the two stale census sentences in `docs/AUDIT.md` carry the correction inline.
