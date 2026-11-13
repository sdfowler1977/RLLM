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
