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

### The run log on the committed bytes (commit `9ba20fee`, room `/tmp/room-bf1`)

A `git archive` room of the committed bytes re-ran everything that matters, with `tooling/` symlinked and no `output/`:

| row | measurement |
|---|---|
| room python census | `py files=4 py lines=243`; `verification/binary_flow.py` **absent** |
| tool gate inside the room | `tools_rc=0 rows=9`, `output/tools/package-tests cases: 46`, `twin helper tests … cases: 44` |
| group twin compile | `compile_rc=0 errors=0 warnings=0` |
| direct run, no comparator | rc 101 at `verification/twin/mod.rs:249` — `could not start …/output/pre-branch-controller`: the twin spawns whatever path it is handed, so a bare direct run fails loudly and writes no report |
| direct run, `--previous-flow-controller output/published-controller` | rc 101 at `verification/binary_flow.rs:2260` naming `actual previous controller retains its original decoder meaning` — the leg's own check, no report written, exactly as the live pair behaved |
| sanctioned route `output/harness binary_flow` | `route_rc=0`, `binary_flow UNAVAILABLE (not executed, missing: output/pre-branch-controller …; output/pre-auth-controller …)`, `summary: 0 pass, 0 fail, 1 unavailable of 71 groups`, `python mentioned in the route log: 0` |

That UNAVAILABLE shape is what the delivered record already carries (`checks.binary_flow`: `code: null`, `recorded_as: 0`, `missing` the same two reference paths). The route did **not** rewrite the record: the room's `verification/RESULTS.json` still had its git-archive mtime `10:38:02` while the route log is `10:44:06`, and the runner's closing `recorded in verification/RESULTS.json: 71 groups, 4248 checks` line reads the record present at run start. The room's first attempt is kept as an instrument note: before I built the room's own controller, the direct run died on `could not start …/output/headless/RLLM` — a missing input in my room, not a verdict.

One thing this commit understated in its own message: the manifest pins the deleted filename. `PUBLICATION-MANIFEST.json` keeps `/files["verification/binary_flow.py"]` (36026 bytes, sha `955991d3ae9d…`), and the publisher dry run on these bytes prints `build_inputs: pinned 162 match 117 drift 44 absent 1` and `files: pinned 281 match 156 drift 54 absent 71` before `refusing: output/checks/apk.json absent` with the destination left empty. So the mentions left in the tree are `closure_ledger.json` (11), `docs/AUDIT.md` (4), `project-progress.md` (2) and that manifest pin (1) — the manifest is regenerated by a real publish, which stays owner-gated on the signed companion APK.

### A TLS lane that would remove the last python site — adjudicated, left uncommitted

Another lane replaced the `python3 ssl` fixture inside `verification/snapshot_identity.rs` with a hand-written native TLS peer: `mod tls` with SHA-256/HMAC/PRF, RSA PKCS#1, AES-128, AES-GCM, X25519, a `Vec<u32>` bignum with Montgomery modular exponentiation, record layer and handshake, plus `mod tls_tests` (13 tests). Measured: HEAD 828 lines → worktree 3245 lines; both versions compile with `0 errors 0 warnings`. It is the site that would take the `Command::new("python")` census from 2 to 1.

It is **not committed**, and the reason is evidence, not taste. My first two-column rooms proved only that both versions compile, because both rooms lacked `output/headless/RLLM` and the runs panicked on that missing input. The audit seat then held it at **NEEDS-PROOF**: RSA PKCS#1 v1.5 has no published known-answer vector (its tests are round-trips plus a Montgomery reference), `the_events_file_keeps_the_shape_the_leg_wrote` pins a shape the same author wrote rather than the delivered record's contract, and a green JVM run proves only that *this* peer and *this* subject interoperate — a shared protocol misunderstanding would pass. The named missing measurement is an independent TLS implementation (an OpenSSL peer or a wire trace parsed by a conformant implementation) verifying the native peer's handshake and encrypted records. It also rejected the framing that the R-rule *requires* the swap: the rule permits python where compatibility forces it, so deleting the last python site is not by itself a reason to land 2,417 lines of hand-written TLS in the instrument. The lane's work is kept in the worktree, unstaged and un-reverted.

### What may be deleted next, and what may not

The four python files left in `verification/` are not four equal cases:

| file | line count | decision | why |
|---|---|---|---|
| `apk_alignment.py` | 54 | delete next | its twin exists and is route-preferred; a clean-room leg-vs-twin run (exit, printed channels, report bytes) is achievable on this host |
| `turn_background.py` | 75 | delete next | same |
| `history_migration.py` | 43 | **keep** | its comparator is owner-supplied and absent, so a leg-vs-twin comparison there compares two *unavailable* reports — the seat's word is exactly right: "comparing two unavailable/unexecuted legs would be vacuous" |
| `live_web.py` | 71 | **keep** | it is in `OPTIONAL`, not `GROUPS`; the `--live` route performs real public HTTPS searches, so the twin must first be shown to be selected by that route and compared on decision/status behaviour under the same network conditions |

Ceilings move only with an approved deletion: `4/243 → 2/114` after the two deletions, then `1/71`, then `0/0`.

That last plan line was optimistic about `live_web`, and the reason is now measured rather than assumed. Run the way its leg runs, the twin refuses: `twin_rc=2 twin_last_line=error: the following arguments are required: --live`, while the python leg runs and returns a report. And `--live` appears nowhere in `tools/harness.rs` — the runner lists `live_web` in `OPTIONAL` and never executes it. So its pair witness cannot be run on this host at all today, and the file stays until someone builds the route, not until someone writes a twin. `history_migration.py` stays for its own separate reason: its comparator is owner-supplied and absent.

## Batch 3: the two approved retirements, and the TLS peer that had to pass a stranger first (2026-11-13)

### Two legs retired on byte-identical pairs, not on a twin existing

| group | pair witness | report | counts |
|---|---|---|---|
| `apk_alignment` | room `/tmp/witq7-apk`, `headless_rc=0`, `tools_rc=0 tools_rows=9`, leg rc=0 / twin rc=0 | `CMP-FILE: byte-identical`, sha `36ff8454aa662cb2bd6280738e00bf2759158de53c46a926b78685547816509b` both columns | 11 checks / 11 passed both sides |
| `turn_background` | room `/tmp/witq7-turn`, same instrument | `CMP-FILE: byte-identical` **and** `CMP-STDOUT: byte-identical`, sha `269f3240961f8d23…` | 28 / 28 both sides |

The `apk_alignment` stdout is *not* byte-identical and does not claim to be: it differs at byte 59 by exactly two lines, python's own `UserWarning: Duplicate name: 'AndroidManifest.xml'` from its `zipfile` module. That is the interpreter talking, not the checks. The audit seat objected that neither log shows *which* APK both columns read, and it was right that the log did not — so the gap was closed by measurement instead of by argument: `twinwitness4.sh:18` exports `RLLM_CHECK_APK=/home/shawn/RLLM/output/2026-11-13__11-33-43Z/RLLM-unsigned.apk` (sha `bd632e712bd754bf`) at script scope, so both columns inherit one path; unsetting it and re-running the twin gives rc=101 with the refusal sentence `refusing to invent an artifact: PUBLICATION-MANIFEST.json delivers RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk as a companion file that is not on this machine; set RLLM_CHECK_APK to the APK under test`; setting it back with `CLIENT_TOOLS` present gives rc=0 and 11/11. A twin that refuses rather than inventing its subject is the behaviour the leg had, now carried natively.

### A latent defect in a twin that only retirement could expose

`turn_background.rs` had a JSON reader that never actually parsed the delivered record: a nested container was sliced to the end of the document instead of at its own closer, and the string branch stripped the opening quote before handing text to the string scanner. While its python leg existed, nothing in the twin's passing path needed that parse, so the defect was invisible. Pointed at `PUBLICATION-MANIFEST.json` it panicked `could not parse PUBLICATION-MANIFEST.json` instead of performing the documented refusal. Tests were written first and measured red on the unfixed bytes (`the delivered record did not parse: expected a JSON string`, 57 other tests green), then green after a `json_container` string/escape/depth-aware scan replaced both container branches. The group test binary now reports **59 passed / 0 failed**, and the no-APK arm prints the retired leg's sentence verbatim (rc=101). Retiring a leg is therefore not only a deletion: it is a way of finding out what its twin never had to do.

### Census, ceilings, and what the runner itself says

Measured after the deletions: `verification/` python is **2 files / 114 lines** (`history_migration.py` 43, `live_web.py` 71); tracked tree **3 `.py` / 229 lines** (`tools/package.py` 115 of them); `const GROUPS` is 71 names, 71 unique, and **70 of them have no `verification/<name>.py`**. `tools/package.rs` ceilings moved `4/243 → 2/114` at every site — both `assert_eq!` pins, the over-ceiling fixtures (`files 51 exceed the recorded ceiling 2`), the lines-pair fixture (115 computed against ceiling 114) and the real-tree census sentence `verification files 2 lines 114 ceiling files 2 lines 114;` — and the over-ceiling arms still go red, so the ratchet did not quietly become a tautology. Gate on these bytes: **`GATE22-TOOLS-RC=0`** (alignment 2, readback 24, checkpoint 5, signature 5, apk 6, toolchain 10, gate 18, package **46**, harness 28), `twin helper tests … cases: 44`, **`GATE22-VERIFY-RC=0`**. The `Command::new("python` census in native code is now **1**, the runner's own leg fallback at `tools/harness.rs:1120`.

### The TLS fixture had to interoperate with a client it did not author

The lane that replaces `python3 ssl` inside `verification/snapshot_identity.rs` was held at NEEDS-PROOF in the last entry, with one named missing measurement: a conformant implementation *outside the fixture* verifying the native peer. That measurement was run, and it went **red first**. `openssl s_client` against the native peer failed all three arms — `error:0A000152:SSL routines:final_renegotiate:unsafe legacy renegotiation disabled`, a fatal `handshake_failure`, and the fixture's own event file recording `captured: []` while the python arm recorded the request — against HEAD's python fixture, which passed the same three arms. Two halves to the defect, both RFC 5746:

1. **Indication.** Section 3.3 lets a client signal secure renegotiation with `TLS_EMPTY_RENEGOTIATION_INFO_SCSV` — the value `0x00ff` **inside its cipher-suite list** — instead of the `renegotiation_info` (0xff01) extension, and OpenSSL 3.0 sends only the SCSV. The lane parsed the extension and never looked at the suite list, so it treated every OpenSSL client as renegotiation-unsafe. Now: `pub const SCSV_SECURE_RENEGOTIATION: u16 = 0x00ff` (`:1180`) and `cipher_suites.contains(&SCSV_SECURE_RENEGOTIATION)` in `ClientHello::parse` (`:1250`).
2. **Echo.** Section 3.4 requires the ServerHello's `renegotiation_info` to carry exactly one `0x00` byte (an empty `renegotiated_connection`). A first attempt emitted zero-length `extension_data` and was measured red with `tls_parse_stoc_renegotiate:renegotiation encoding err` before the byte was put back.

Final: all three arms rc=0, the full record sequence `ClientHello → ServerHello → Certificate → ServerKeyExchange → ServerHelloDone → ClientKeyExchange → CCS`, and `CMP-events: identical` — the native fixture captured the same HTTP request bytes as the python fixture under the same third-party client. Against the group's own JVM subject the repaired peer is still `CMP-REPORT: byte-identical`, 61 checks / 61 passed in both columns (head sha `011c4a6b3f172c46` / 828 lines, lane `f5e481b3ad91756b` / 3,234 lines), each room building its own headless controller. The new checks discriminate: CONTROL 57/57, and four mutations (SCSV detection removed, SCSV constant `0x00fe`, echo length `0`, echo omitted) each go red naming `tls_tests::a_client_that_offers_only_the_scsv_is_answered_with_an_empty_renegotiation_extension`, each arm restored byte-identically (`final_sha_match=yes`).

Code that only tested itself was deleted rather than kept as coverage: `rsa::encrypt_pkcs1`, `rsa::decrypt_pkcs1` and the `Random` generator. `git grep` over tracked `*.rs` finds **zero** occurrences of all three, and HEAD's version never contained them — they lived only in this uncommitted lane, reachable only from their own round-trip tests. The RSA path that is live is `sign_pkcs1_sha256` (defined `:980`, called by the ServerKeyExchange path at `:1583`), and *that* signature is verified by an implementation outside the fixture, twice over: OpenSSL completes the handshake with it and the JVM accepts it. The group is 3,234 lines with 57 unit tests, 0 errors, 0 warnings, and spawns nothing.

### A false green in the runner, found while refusing to trust a sub-agent's reds

A census sub-agent reported `gateway_expectation` and `service_identity` red. Re-measured myself: `gateway_expectation PASS (exit 0, 0s, 26 checks)`, `rrsi_loop PASS (exit 0, 1s, 15 checks)`, and `service_identity` **does not exist** — not in `GROUPS`, no `verification/` file. Chasing that invented name is what exposed the defect: `output/harness <name>` selects by filtering `const GROUPS` (`tools/harness.rs:1829`), so a requested name the runner will never run matches nothing, prints no per-group line, and still prints `summary: 0 pass, 0 fail, 0 unavailable of 71 groups` and **exits 0**. Measured: `output/harness zzz_nogroup` rc=0 with no `output/checks/zzz_nogroup.json`; `output/harness live_web` rc=0 the same way, even though `live_web` is declared in `OPTIONAL` and the runner has no `--live` route at all. The census already refuses undeclared *files* (`:325`); nobody ever wrote the symmetric guard for requested *names*. That matters because every GAP run log is claimed through this runner — a mistyped or retired name would have looked like a pass. Repair is in flight on `tools/harness.rs` alone, and it lands only with tests measured red on the pre-fix bytes plus mutations naming one red each.

### The guard, closed on a two-column witness (same day)

The repair is in, and it was witnessed against HEAD rather than argued: `/tmp/guardwit.sh` builds the runner twice — HEAD's `tools/harness.rs` (sha `c36f6c05f5e3058e`) and the guarded bytes (`c212a14e94b7f7a0`) — and runs the same four probes in both columns.

| probe | HEAD column | guarded column |
|---|---|---|
| `zzz_nogroup` | rc=0, no stderr, `summary: 0 pass, 0 fail, 0 unavailable of 71 groups`, no report file | rc=2, `unknown group zzz_nogroup (not a runner group, a declared helper or a declared optional group)`, **no** summary line |
| `live_web` | rc=0, same silent shape | rc=2, `live_web NOT EXECUTED (optional group: requires --live: performs real public HTTPS searches)` — the reason string is `OPTIONAL`'s own text (`reason: *reason`), not a copy |
| `gateway_expectation` | `PASS (exit 0, 0s, 26 checks)`, rc=0 | byte-identical, rc=0 |
| `gateway_expectation zzz_nogroup` | rc=0 | rc=2 with **0 stdout lines** — nothing executes before the refusal |

The new cases were mutated by me, not by the lane: CONTROL 32/32; unknown-name branch → two named reds; `OPTIONAL` match neutralised → `checks::live_web_is_refused_with_the_optional_reason`; and one honest instrument failure — my first prefix mutation (`name.starts_with(group)`) stayed **green** because that direction can never change a verdict (a short name does not start with a longer group name), so it was a bad instrument, not a test gap, and was re-run as `group.starts_with(name)`, which went red naming `checks::gateway_prefix_is_refused`. Every arm restored byte-identically. Gate: `GATE23-TOOLS-RC=0` with `output/tools/harness-tests cases: 32` (was 28) — the case-count interlock moving with the new tests.

The audit seat (`/tmp/mm-audit-guard.out`) returned **VERDICT: REJECT** on two objections that collapse against their own quoted lines. On A1 it wrote *"The head column exits 0 and prints a summary, so the claim's required condition is not met"* — while A1's claim **is** that HEAD exits 0 and prints a summary; that is the defect, and the line it quotes is the proof. On B1 it could not "prove" a row the document marks NOT MEASURED, which is not a claim the packet owes evidence for. The rest (A2–A6, B2–B3) was accepted.

### A measured re-baseline of the gap report

The yardstick document is dated 2026-11-11 and describes its own method as static — *"Every finding comes from reading the current RLLM source tree, not from running it."* Several of its ranked gaps have moved, and two of its "missing entirely" claims are now contradicted by measurement, so `docs/RLLM-gap-status-2026-11-13.md` records the status row by row with the instrument line that decided each one, and keeps unmeasured rows labelled **NOT MEASURED** instead of rounding them up.

What changed materially: **Gap 2** is no longer the five "thin" bullets the report lists — the loop takes one to three rounds (`core/improvement.rs:113-121`, with `three_refinement_rounds_are_accepted_and_four_are_refused` pinned at `:893`), edits eight named harness components rather than instructions only, and its safeguards are wired and asserted: `rrsi_loop` PASS 15 checks asserts budgets annealing **4→3→2**, a **leakage refusal before scoring**, **noise floor 2**, **pruned skills**, the **evolve-tip incumbent commit in `.rllm-worktrees`**, and the **four held-out keys**, plus the cost arms (`a shorter equal score is accepted for lower cost, and the same shape is refused when the cost rose`) and the stall arm (an already-tried component refused, an untried one not). **Gap 4** has `core/js_evidence.rs` (2,532 lines), `core/dotnet_evidence.rs` (3,428) and `core/har_evidence.rs` (1,808), and `pe_declarations` PASS 43 checks asserts `cited clean-room behavioural specification`, `clean-room artifact provenance names the behaviour document digest` and `clean-room implementation generated from cited behavioural claims` — so the white-room rebuild exists; what is measurably **absent** is pseudocode (`grep -rl pseudocode core/` → 0 files). **Gap 1**'s `self_rebuild` PASSes 9 checks but has no run log on these bytes.

What is genuinely still owed, in order: the **Gap 2 and Gap 3 run logs on committed bytes** (the capabilities are asserted, but the artifacts are not on file — `rrsi_loop`'s log names the values without printing them), Gap 3's cross-task library read-back (implemented in `historical_experience` → `historical_section_checked`, asserted by no group anywhere), the k-candidate pool and composition of Gap 2 #11 / Gap 3 #2–3, Gap 4's pseudocode, and the Gap 5 residuals (`live_web` needs a `--live` route the runner does not implement at all; `history_migration` needs an owner-supplied comparator; `tools/package.py` and the manifest's stale pins wait on a real signed publish).

### The library read-back, now asserted by the group that owns the loop

Gap 3's decisive sentence is *"task t+1 visibly reading the library entry written by task t"*. The product does it — `historical_experience` (`core/harness.rs:239-248`) calls `rrsi::historical_section_checked` and puts a `HISTORICAL EXPERIENCE` block into the propose request — and `docs/RRSI-LOOP-RUNLOG.md` §Gap 3 witnessed it at the product level on 2026-11-12. **No delivered group asserted it**: the only `experience.jsonl` rows in `verification/` were the sealing row and the tamper refusal. That is now closed by two appended checks in `verification/rrsi_loop.rs` (+214/−1, sha `3d5ebb1f037c3eac`).

The cross-task check drives two refine tasks over the product JSON-lines route: the first settles and seals a row, then `cross_task_read` finds that row in `experience.jsonl` by its `run_digest`, takes its stored `key`/`harness`/`lesson`, and drives a **second task with different task text**, capturing the request wire the product actually sent and the phase it was sent from. `CrossRead::ok()` is thirteen conjuncts: a sealed row exists, the section is present, it carries the product's own relation sentence `indexed key is from another task`, the row's key equals `task_key(TASK)` computed independently by the twin, that row's `task` field is the first task's text, the wire names that key and the clipped harness and lesson bodies, the wire does **not** name the current task's key and no key in the section is the current task's key, the parsed key list contains the earlier key, the capture happened at `phase == "propose"`, and the wire is a proposal request. The negative arm runs a task over a library with no sealed row and requires `rows == 0`, no heading, no `Adopted harness (historical)` line, still at propose.

Fidelity was measured against HEAD in two columns, not argued. From the source: the HEAD binary printed 15 check lines, the lane 17, and `CMP-first-15-check-lines: identical`. Through the runner: `rrsi_loop PASS (exit 0, 1s, 15 checks)` for a HEAD-built twin and `PASS (exit 0, 1s, 17 checks)` for the lane-built twin, with the two reports sharing the key set `[checks, count, failed, passed, scope]`, `count` 15 → 17, `failed` empty in both, `first 15 names identical and in order: True`, and exactly two names appended. The only intentional shape change is `scope`, which the new sentence grows (1,592 chars).

The four mutations are mine, and each landed (per-mutant diff shows one changed line) and went red naming exactly one check: probing the *current* task's key, claiming the sealed row belongs to the later task, and asserting a different relation sentence each stopped at `a later refine task reads the earlier task's sealed experience entry from the propose prompt`; inverting `!self.heading` in the empty arm stopped at `a refine task whose library holds no sealed row shows no historical experience section`. The unmutated control compiled and printed `17 rrsi-loop checks passed`, and the lane file's sha256 is unchanged after the battery (`BASE-SHA == RESTORE-SHA`, `3d5ebb1f037c3eacf42032e50d7d280134dac0cde4dc797ceb2a8bafd47decc3`).

The audit seat (`/tmp/mm-audit-rrsi.out`) returned **ACCEPT WITH NOTES**. Its first note was a real hole in my own evidence, not in the increment: three rows of the fidelity join had failed on a wrong path (`output/checks/rrsi_loop.json` — the report file is written with a **hyphen**, `rrsi-loop.json`), so the report-shape leg was unmeasured at verdict time. It was measured afterwards, and is the runner-report join above. Its second note agrees with the drift measurement below.

### Two of my own measurements were wrong, and both are corrected in the gap-status document

`docs/RLLM-gap-status-2026-11-13.md` said the Gap 2 and Gap 3 run logs were "not on file". They are: `docs/RRSI-LOOP-RUNLOG.md` carries both, with the values printed — budgets 4/3/2 quoted out of the request wire, the leaking candidate refused before scoring with its hypothesis stored as a digest, gain 1 inside noise floor 2, `pruned_components=["skills"]`, the incumbent as worktree commit `e28d8f177ed8c616df9c23003af4df7d2b174a72` with `main` unmoved, and the four held-out keys with digests the driver recomputed. The honest statement is narrower: that log is **stale for these bytes**. `git diff --stat b8b0e71..HEAD -- core/` shows `core/harness.rs` +45 and `core/rrsi.rs` +2, and the change is in the cost arm (`acceptance_policy` gained a `measure_cost` parameter; a dropped-field tie now leaves cost unmeasured at `core/harness.rs:1249-1266`), so the cost row of that log is no longer a witness. The same correction applies to Gap 1 in the other direction: `docs/SELF-BUILD-RUNLOG.md` §"The whole chain in one job, on committed bytes" ran at `57b063f`, and `git diff --stat 57b063f..HEAD -- core/self_build.rs` is **empty** — the product bytes under that log have not moved, so what is owed there is a re-run of the driver, not a missing capability.

Gap 1's delivered group was re-run on current bytes in a `git archive` room: `/tmp/room-gap1` of `d31ec6c` (no `.git`, `verification/` down to `history_migration.py` + `live_web.py`), the room's own `bash headless.sh` → `rc=0`, room-built tool suites, then `output/harness self_rebuild` → `PASS (exit 0, 1s, 9 checks)` — the first attempt there was legitimately red (`self_rebuild UNAVAILABLE (not executed, missing: path output/tools/gate)`) because my room had no built gates, which is a missing input in my instrument, not a product failure. Nine check names captured, including `spawn records a successor pid that is a child of this product process`, `the next self-build reaps that successor and the pid is gone`, and `an omitted job records the carried job as inherited`.

Gap 2 #11 got a real route measurement instead of a guess: a refine reply beginning `candidates:` is parsed by `parse_pool` and handed to `evaluate_pool` at `core/harness.rs:710-711`, which refuses more than `CANDIDATE_MAX` candidates (`:856-858`) and a probe/candidate count mismatch (`:859`), and which on the recorded-worktree failure path marks every pool row `admissible: false` and sets `incumbent_kept: true` with `round_note: "incumbent kept because no candidate was admissible"` (`:867-875`). The only pool row in `verification/` is the **negative** one (`!pool_text.contains("pool_status")` for a non-refine run), so the k-candidate pool remains the largest un-witnessed piece of the recursion story.

Gates on the lane bytes: `bash tools/build-verification.sh .` → **`GATE24-VERIFY-RC=0`**, `twin helper tests … cases: 44`, and the `rrsi_loop` twin rebuilt from the lane (the build also prunes a binary whose source is gone — which is why the retired `.py` legs left no stale binaries behind). Tool gate: see the ledger row. One instrument bug found while measuring: `build-verification.sh` takes the **project root**, not an output directory — `bash tools/build-verification.sh output/verification` globs `output/verification/verification/*.rs`, finds none, prints `verification twins: none` and exits **0**. A green that built nothing.

### A print that says "absent" used to be green; the group now fails on its own log

The `rrsi_loop` group asserted the six RRSI bars but its log only **named** them, so the artifact values were only recoverable from a python driver in `/tmp` — which is what `docs/RRSI-LOOP-RUNLOG.md` is. The lane makes the delivered group its own run log: it prints 27 value lines and the last check reads this run's own printed lines. Concretely: each of the six propose wires is quoted out of the request and printed on the line of the request that carried it (`anneal propose round 1 Edits remaining this round: 4. The annealed budget is 4.`, resumed `… 1. The annealed budget is 3.`, drive 4/3/2); every ledger row of all three runs prints its noise floor (`drive ledger row 5 round 3 component skills noise_floor 2`); the drive run prints `pruned_components ["skills"]` and the flat-skills run prints its absence; the incumbent prints three ways (`… commit a316f829… refs/heads/rllm-evolve a316f829… worktree HEAD a316f829… refs/heads/main 65103240… planted seed 65103240…`); the held-out task and verifier digests print next to the separate evaluation (`separate evaluation base passed 1 separate evaluation candidate passed 0 in-round score_after 1`); and the sealed library row's `key/revision/harness_digest/run_digest` print next to the empty library that preceded it.

Three mutation batteries on three generations of the file, every arm landing verified by sha and the file restored identically (lane sha `42c9429fb46b0c0adf36748f9fe9403b641b0d7c02f701ee38272f86dc730c4f`):
- **Battery A** (`/tmp/rrsiprint.log`, before the log check): arm `a2` broke the wire-search constant and stayed **green at 17 checks** while both budget lines degraded to `propose budget sentence is absent`. That is the defect class — a print can rot and the gate cannot see it. `a1` (budget value 4→5) went red naming `three propose prompts anneal remaining edits from 4 to 3 to 1`, and `a3` (evolve ref renamed) and `a4` (`held_out_verifier`→`held_out_verifierx`) both went red naming `three refinement rounds record budgets 4, 3 and 2, a leakage refusal, noise floor 2, pruned skills, the evolve commit, and held-out keys` — so the `drive_wire_1/2/3` refactor kept the pre-existing assertions biting.
- **Battery B** (`/tmp/rrsilog.log`, first log check): `c1` pruned-set, `c2` held-out key, `c3` evidence count, `c4` the same broken constant as `a2` — all red, each naming the log check.
- **Battery C** (`/tmp/rrsilogic/battery.log`, after the audit notes were turned into assertions): `d0` control green at **18 checks / 27 evidence lines**; `d1` per-request budget attribution, `d2` per-run noise census, `d3` the flat-skills absence, `d4` the empty-library absence, `d5` the print-side label — all red naming the log check.

The second audit seat returned **ACCEPT WITH NOTES** with four notes (F3 presence without attribution, F4 a vacuous `lines.len() >= 25` plus a twin-emitted constant substring, F5 an unattributed `absent` surviving on the flat-skills and empty-library lines, F6 the SCOPE sentence over-claiming "each propose request"). None were argued away: the budget conjunct is now a `(label, sentence)` pair that must share one printed line, the noise conjunct must find floor 0 on each of the three named runs *and* floor 2 on a drive row labelled round 3, the two absences are asserted rather than tolerated, the count conjunct is gone, `separate evaluation` must appear on the same line as the in-round score, and the SCOPE sentence now says what the code prints. Battery C is the proof each one bites. A third audit seat on the strengthened bytes returned **ACCEPT WITH NOTES** again, with two notes carried forward honestly: the log check certifies that each named line carries the expected value **with its own label**, but it is not a provenance certificate — it does not re-derive each printed value from a second, independently captured copy of the artifact, and `pruned_components ["skills"]` and the separate-evaluation pair remain twin-held expectations. The provenance rests on the printers reading live artifacts (request wires, the digest-named JSONL ledgers, `.git` refs and worktree HEAD, `experience.jsonl`) and on the three batteries reddening when those derivations are broken — not on the final check alone. That seat's second note was that the transcripts I supplied did not prove `verification/RESULTS.json` was untouched by the runner; the proof is a hash join, not a claim: worktree hash before and after the run equals the `HEAD` blob (`git hash-object verification/RESULTS.json` = `git rev-parse HEAD:verification/RESULTS.json`). Its third precision point — that the resumed row bound only the annealed tail, not the `Edits remaining this round: 1.` value that distinguishes 3/1 from 3/3 — became a real binding: all six budget rows now require the full wire sentence, arms `e1`/`e2` red naming the log check. Gates on the final lane bytes (sha `a9c3e82df860b1af5f7906372a26a5e7bacbe9f6cd08cf9b10c201347f37588e`): `bash tools/build-tools.sh output/tools` → **TOOLS-RC=0** with suites 2/24/5/5/6/10/18/46/32, 72 verification twins and `twin helper tests … cases: 44`; `bash tools/build-verification.sh .` on the pre-`e1/e2` lane → **VERIFY-RC=0**, 72 twins, 44 helper cases; `output/harness rrsi_loop` → **`PASS (exit 0, 1s, 18 checks)`** with 27 `evidence` lines in `output/checks/rrsi_loop.log`, and the runner left `verification/RESULTS.json` byte-identical — `git hash-object` before `1450f95863f9cc05b3c61e95ce7941d7f3154855`, after `1450f95863f9cc05b3c61e95ce7941d7f3154855`, `HEAD` blob `1450f95863f9cc05b3c61e95ce7941d7f3154855`, `untouched=yes`.

Two more gap-document rows were corrected by measurement rather than rephrasing. **Gap 4 #2** said "pseudocode ABSENT" on a token grep: the token is absent, but `core/instruction.rs` (536 lines) decodes AArch64 words and `analyze` (`:329`) emits `AArch64 instruction IR, candidate blocks and local register data flow` with one `Statement{offset,address,word,block,mnemonic,text,path_facts}` per instruction (`:25`), wired into the product at `core/control_flow.rs:290`; the registered group `binary_flow` asserts instruction facts (`CBZ target and register retained`) over subjects it assembles with `aarch64-linux-android26-clang`, while `grep -rn "instruction IR\|candidate blocks\|data flow" verification/*.rs` → **0 hits**, so the IR text itself is uncertified. **Gap 3 #2/#3/#6/#7/#8** said "NOT MEASURED" for the deliverable/reflection/invariant/domain layer; the sealed row vocabulary settles it — `EDIT_KEYS` (`core/rrsi.rs:58-69`), `POLICY_KEYS` (`:70`) and `SPLIT_KEYS` (`:73-91`) carry no such field, and `deliverable`, `reflection`, `invariant`, `domain_scop` appear in **0** files under `core/` and `verification/`. That layer is absent, not un-witnessed; `resume_run` (called at `core/harness.rs:283-285`) is the implemented half and is asserted.

### The k-candidate pool is now a check target, and the product contradicted two of my assumptions

`verification/rrsi_loop.rs` had exactly one sentence about the candidate pool — the negative one, `a non-refine
carries no workspace and does not attempt the pool`. The pool is now driven through the product JSON-lines route
by five checks. That commit's diff is `3 files, +591/−1` —
`verification/rrsi_loop.rs` `+373/−1`, `closure_ledger.json` `+153/0`,
`project-progress.md` `+65/0` — so what that commit omits is a `core/` change,
python, and any new file. One ledger row per candidate, each row
stamping the digest of the journal it is read from; two malformed-count refusals carrying the product's own
sentences; a fully refused pool keeping the incumbent byte-equal to its instructions; the three slot worktrees
that round leaves behind holding one candidate each; and the group's own log check extended so it consumes that
new evidence instead of merely allowing it. Group count 22 → 23, report `['checks', 'count', 'failed', 'passed', 'scope']` with `failed` empty.

Three things I believed about the product were wrong, and measurement is what said so.

1. **One harness surface, when there are two.** Audits 1 and 2 both accepted a claim about "the harness file".
   The run log shows the room's own `rllm-harness.txt` staying `{"a":""}` through a fully refused pool while a
   *candidate worktree* copy gets overwritten with a refused candidate's body — so "the harness did not move" was
   unfailable on one path and false on the other. The claim now names one slot (`c0`) and three values.
2. **An order-independent claim that was not.** The first two audits accepted a slot-copy conjunct pair; arms
   `t1`/`t3`/`t4` showed which parts actually bite, and the pair was last-writer-wins coupling on product
   scheduling state. Narrowed to one named slot and three named values.
3. **A print that described a filename, not a join.** The journal summary used to print `joined present` from
   `is_hex(digest, 64)`. It now counts rows whose own `run_digest` stamps that journal, which is what let arm
   `v7` plant the run's own three-row journal under an all-zero digest name: the log then prints
   `pool decision rows journal 0000000000000000000000000000000000000000000000000000000000000000 rows 3 stamped 0`
   and the run goes red naming
   `a three-candidate pool writes one ledger row per candidate and stamps each decision`.

The per-candidate worktree clause of Gap 2 #12 went from "inferred from source comments" to observed. The run
prints, from the committed bytes:

```
pool slot c0 row_commit c7a733ba7179e6098bd27cd3c38c67618e891452 worktree_head c7a733ba7179e6098bd27cd3c38c67618e891452 slot_git present harness_text {"a":""}
pool slot c1 row_commit 234ee79ed76e2dafb17505f5ed9939665f844566 worktree_head 234ee79ed76e2dafb17505f5ed9939665f844566 slot_git present harness_text Copy the task Drive three rounds. into the harness
pool slot c2 row_commit 3f29f439e712ae69212ea1f2cc22dee2dc361412 worktree_head 3f29f439e712ae69212ea1f2cc22dee2dc361412 slot_git present harness_text component: skills hypothesis: flat one flat body one
```

Three distinct commits, each slot HEAD equal to its row's commit, each slot's `rllm-harness.txt` equal to the
candidate body submitted at that index. The product explains why that is the shape: `commit_candidate` at
`core/rrsi.rs:1969` takes the slot index, removes any stale slot, adds a detached worktree at
`slot_path` = `.rllm-worktrees/c{slot}`, and writes that candidate's body into the slot's harness copy;
`core/harness.rs` calls `slots.keep()`, so the slots survive the round. What is *not* claimed: simultaneity
during `evaluate_pool` (the commits are sequential), and candidate-to-row identity beyond commit and body
binding — the product emits no candidate id. That is recorded in the ledger row rather than papered over.

**Falsifiability.** Seven mutation batteries on uncommitted lane bytes (the seven logs end in six distinct restore digests), re-parsed while the committed lane was the checked-out file — the seven logs end in six distinct restore digests, none of which is a committed blob, so the runs were made on uncommitted lane states: 58 `== arm` records — 45 ran `rc=101` naming the check they broke (31 via `named='<check>'`, 14 via
`panic:` plus `last leg: leg <check>`), 10 green, 2 `SKIP landing hits=2`, 1 build failure
(`r9 compile_rc=1 errs=2`). 56 of 58 carry a `-- landing <id>: … sha=` line; the two that don't are the SKIP
arms. Per log (`== arm` / `-- landing` records): 7/7, 9/9, 17/15, 3/3, 7/7, 7/7, 8/8 — the two missing
landings are the q9 and q14 SKIPs, both in `poolmut3.out`. Each log ends `RESTORE-SHA <sha> unchanged=yes`, computed against that battery's pre-mutation bytes. The
10 green arms are 6 zero-change controls (`p0`, `p0`, `q0`, `s0`, `t0`, `v0`) and 4 mutants recorded as
non-certifying rather than as passes: `s2` (the state root's first `.jsonl` at that point was that run's own
journal), `s6` (the flag it forced was a derived print), `t2` (the removed claim pinned nothing else by
construction) and `t4` (the live disjunct in that run was the candidate-body one). The last battery's restore digest is `daa9f5a3…`, so that battery ran on lane bytes of that generation; no battery ran on a committed lane state: the parent lane blob hashes to `a9c3e82df860b1af…` and the committed lane blob to `c6994ce5b53eb4ab…`, and none of the seven logs' six restore digests is either of them. The difference between the last battery base and the committed file is seat 5's reading of the battery's own base copy (the SCOPE string and one comment line, six changed lines because the SCOPE constant spans lines); no git object holds that base, so that diff is an attestation, not something this witness re-runs.

**Audits.** Five seats; the ledger row's `audits` field records each seat's disposition and findings (transcripts on disk, sizes measured by `wc -lc`: `/tmp/mm-audit-pool.out` 28,320 B / 635 lines, `-pool2` 17,570 / 323, `-pool3` 19,508 / 405, `-pool4` 30,085 / 667, `-pool5` 2,210 / 30 — 97,693 B and 2,060 lines, `wc -lc` on each): four ACCEPT WITH NOTES on successive byte sets, then ACCEPT with no
notes on `c6994ce5b53e`. The ledger row's `audits` field records what each note produced: the three product corrections above, a redundant conjunct removed, a post-reply run-digest assertion added, and the SCOPE printed-values inventory rewritten to exactly the set the log check consumes.

**Gate.** A git-archive room of `192d6aa` with these exact lane bytes and this ledger overlaid, compared with
the commit tree path by path: identical path sets (354 / 354, `comm` empty both ways) and 352 of 354 files
byte-identical to their committed blobs. The only two differing files are `closure_ledger.json` (the
working-tree copy the room script overlaid on 192d6aa2's `5e65deeeafce…`) and `project-progress.md` (never
overlaid: the room holds 192d6aa2's own committed copy, digest `2bdc3c5ced4f…`, which `065efd3` later
rewrote), so the room is not a byte-copy of the commit. That join was re-measured on 2026-11-13 by `/tmp/g30joinnow.py` (artifact `/tmp/g30join-now.txt`, sha256 `b64bbf5c2a12…`): the path sets are identical to both `192d6aa2` and `065efd3` (`only_room=[] only_tree=[]` against each, 354 paths three ways), `352` of `354` blobs identical against the base and `352` of `354` against the lane. Against the base `192d6aa2` the two differing files are `closure_ledger.json` (room blob `d49f323cf53c`, base `a00e23c9c70e`) and the overlaid `verification/rrsi_loop.rs` (room blob `955a094d12c3` = `065efd3`'s blob, base `6964c848c3cd`); against the lane `065efd3` they are the two named here. The 12-hex digests quoted in this section are sha256 prefixes, not `git hash-object` blob ids; the artifact prints both kinds. Inside the enumerated compile closure neither document can move a verdict: the four verbs compile `core/` (125 `.rs`), `verification/` (72) and `tools/` (9); the only include directives in those roots are `core/desktop.rs:4` and `core/console.rs:2` (both `include!("lib.rs")`) and a string literal at `verification/self_rebuild.rs:1861` in the working tree (`:834` at `065efd3`) that becomes `include_str!("../src/edited.rs")` in a scratch crate built at run time — `grep -rn "include_str!\|include_bytes!\|include!" --include='*.rs' core tools verification` = 3 sites, none a claim document; the reader census over the same roots (`--include='*.rs' --include='*.py' --include='*.sh'`) is `closure_ledger` 0 files, `RRSI-LOOP-RUNLOG` 0, `RLLM-gap-status` 0, `project-progress.md` 2 files — `tools/package.rs:2674`, `:2911`, `:3054`, `tools/package.py:77` — and `package` is a verb this gate never ran. For the committed bytes that closure argument is moot: a byte-copy room (`/tmp/g39room.sh`, `extract: git -C /home/shawn/RLLM archive 065efd31121593ab51bd89a151dd5f7df4ae72c2 | tar -x -C /tmp/g39room ; ln -s /home/shawn/RLLM/tooling /tmp/g39room/tooling`, nothing overlaid, `room_has_git=no`, transcript `/tmp/g39room.out`) holds all 354 tree paths, 354 compared and 354 identical to their committed blobs (`git hash-object` vs `git rev-parse 065efd31121593ab51bd89a151dd5f7df4ae72c2:<path>`), including 235 of the 235 files under the roots the four verbs compile; the only non-`output/` extras are the 6 logs I redirected into the room root. In that room `output/harness rrsi_loop` → `rrsi_loop PASS (exit 0, 2s, 23 checks)`, report count 23, `failed` empty (`/tmp/g39room/rrsi_loop.log`), and the full sweep there is `sweep_rc=1 summary: 64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4154 checks` over 71 per-group rows, the single failing row being the pre-existing `history_migration` group (`history_migration FAIL (exit 101, 0s, absent checks)`) — because that reference is never produced on the measured route: the only *writes* of `output/previous-controller` in tracked source are test bodies at `tools/harness.rs:2488` and `:2509`, inside `#[cfg(test)]` which begins at `:1986`; the only other tracked references are the input declaration at `:56` and the default-path construction (`:1085` in the working tree, `:1077` in `065efd3`); and no build script in the measured set mentions the path (`grep -rn previous-controller tools/*.sh headless.sh package.sh desktop.sh` = 0 hits); `leg_source` reads `verification/<group>.rs` when that file exists and falls back to `.py` only otherwise, so for this group the input detector reads the **native** twin; the twin states its requirement in its own parse error (`verification/history_migration.rs:73`, `the following arguments are required: --previous-controller`) while the committed detector looked only for a script's `add_argument('--previous-controller'` declaration, so the requirement was invisible to it; the harness then hands the twin its default path (`tools/harness.rs:1085` in the working tree, `:1077` with the push at `:1079` in `065efd3`; the input is declared at `:56`) and the twin spawns that path as a controller binary and panics on the missing file — the room log carries the panic verbatim: `could not start /tmp/g39room/output/previous-controller: No such file or directory (os error 2)` (`/tmp/g39room/output/checks/history_migration.log`); the panic site is `verification/twin/mod.rs:243-249`, `panic!("could not start {}: {error}", binary.display())` inside `spawn_at`. The delivered record disagrees with that red, and the disagreement is dated rather than argued: the record's `connected.unavailable` entry {"name": "history_migration", "code": null, "missing": ["--previous-controller reference at output/previous-controller"], "recorded_as": 0} (and its `checks` entry, which adds "history_migration was not executed because a development input is missing: --previous-controller reference at output/previous-controller.") was published at `0eb0cb1` (2026-11-12 19:38Z) and the native twin landed later at `54c1f1c` (2026-11-13 10:22Z); `git merge-base --is-ancestor 0eb0cb1 54c1f1c` → true, so that entry describes the python-leg era. A working-tree harness fix now reports that group `UNAVAILABLE` with its missing-input reason instead of failing: on the same lane bytes the committed harness sweeps `64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4156 checks` and the fixed harness sweeps `64 pass, 0 fail, 7 unavailable, 0 absent of 71 groups, 4156 checks` (`/tmp/g32room/output-sweep.log`, `/tmp/g36sweep.log`, `/tmp/hmfix.out`); keyed by name, verdict and check count, exactly one row of those two rooms differs — `history_migration FAIL (exit 101, 0s, absent checks)` against `history_migration UNAVAILABLE (not executed, missing: path --previous-controller reference at output/previous-controller)` — and both rooms' per-group sums are 4156. The publication gate is measured on the same bytes, not assumed: `commit_sweep` (`065efd3:tools/harness.rs:1616`) returns before `publish_sweep` when any row failed and prints `delivered record left untouched: a group failed`, which is what this room printed (`delivered record left untouched: a group failed`); its `verification/RESULTS.json` is byte-unchanged, before and after the sweep (`results_before=1450f95863f9cc05b3c61e95ce7941d7f3154855 results_after=1450f95863f9cc05b3c61e95ce7941d7f3154855 results_committed=1450f95863f9cc05b3c61e95ce7941d7f3154855`). The zero-failure room on the same lane bytes writes its own record — `delivered record written: 71 groups, 4156 checks` (`/tmp/g36sweep.log`) — and its record's git blob id moves from the committed `1450f95863f9cc05b3c61e95ce7941d7f3154855` to `3887aaab73138408910de1461ba427a351f01dc7` — both are blob ids of `verification/RESULTS.json` (`git rev-parse <rev>:verification/RESULTS.json`), and their sha256s are different numbers (the committed file hashes to `e2f35542e44b…`), in a directory that is no git repository at all (`room_has_git=no` here, and `git -C /tmp/g36room rev-parse HEAD` → `fatal: not a git repository`). The sentence this document carried before — that publishing needs a git repository and a git-archive room has none — was wrong. The byte-copy room is also the arity witness this lane turns on: there `self_rebuild` is `self_rebuild PASS (exit 0, 1s, 9 checks)`, while the lane rooms report `self_rebuild PASS (exit 0, 2s, 11 checks)` — 4147 more checks in that group. Those 11 checks are that lane's count at those two room runs; the lane file now carries a twelfth check, `a review permit refuses a same-revision decoy commit and commits the tracked source tree above it`, and a a `git archive` room of `065efd3` with the three lane files copied over it (an overlay room, not a byte-copy room) runs the group green at `12 self-rebuild checks passed` (`/tmp/g42room-out/self_rebuild-baseline.log:18`), with the mutation arms that show the product's commit-tree guard is what makes that check pass recorded in `docs/RLLM-gap-status-2026-11-13.md`. Comparing all 71 per-group rows of the two rooms (`/tmp/g39-verdicts.txt` against `/tmp/g32room/output-sweep.log`), keyed by group name with exit code and elapsed seconds dropped, changes exactly one row, that one. The whole-tree sums differ by 2 checks (4154 here, 4156 there), which is that group's difference and nothing else. That room is not claimed green as a whole tree — its four build verbs and its `rrsi_loop` run are rc=0 (`headless_rc=0`, `verify_rc=0`, `tools_rc=0`, `harness_build_rc=0`, `runner_rc=0`), its sweep is `sweep_rc=1` — `64 pass, 1 fail, 6 unavailable, 1 absent of 71 groups, 4154 checks`. The room path moved from `/tmp/g35room` to `/tmp/g38room` and then to `/tmp/g39room` because the earlier paths were reused mid-session (the second transcript was truncated by a second launch on the same output path); every figure here is read out of `/tmp/g39room.out`, `/tmp/g39-verdicts.txt`, `/tmp/g32room/output-sweep.log` and `/tmp/g36sweep.log` by the script that rewrote this sentence, not transcribed. The g30 room above stays the witness for the pre-commit lane bytes, which is what it was built for: g30 is
an overlay room (`192d6aa2` + the pre-commit lane file + the working-tree ledger), g39 is a byte-copy room of
`065efd3` with nothing overlaid (earlier rounds used `/tmp/g35room`, then
`/tmp/g38room`; both paths were later reused, so `g39` is the live witness), and every claim below says which one produced it. grok's in-flight `verification/self_rebuild.rs` (not in this commit) is not in it: `headless_rc=0` with zero warning lines, `verify_rc=0` (72 twins,
`twin helper tests … cases: 44`), `tools_rc=0`, `harness_build_rc=0`,
`runner_rc=0` → `rrsi_loop PASS (exit 0, 2s, 23 checks)`; `yes` on `verification/RESULTS.json`. An earlier in-tree
gate showed `tools_rc=1` from a *concurrent* writer's half-built `self_rebuild.rs`
(`error[E0425]: cannot find function 'room_handoff' … self_rebuild.rs:100`); that red was not mine and was not
argued away — it is recorded with its attribution in the ledger row. The gate for the committed bytes is the g39 byte-copy room transcript (`/tmp/g39room.out`: 354 of 354 tree paths byte-identical to `065efd3`, every verb run there); the g30 overlay room is the witness for the pre-commit lane, where two of the 354 files differ from `065efd3` because the room script overlaid one and never copied the other.

## Record-lag close: units landed since the previous entry (written 2026-11-17T08:54Z)

This document last moved at `629683b` (2026-11-13T18:18:16-05:00). Since then `git rev-list --count 629683b..HEAD`
= **45** commits, of which **20** change paths under `core/`, `tools/`, `verification/` or `platform/`. The list
below is read from `git log --format=… --shortstat 629683b..HEAD -- core tools verification platform`, not from
memory, and each row is the commit's own shortstat:

- `6102133` classify a native group's own required reference as a missing input — 1 file, +27/-2
- `e500c6d` commit the journalled revision on the product's own verb — 3 files, +1393/-14
- `46e392e` carry a journalled revision forward on the re-exec verb — 2 files, +327/-1
- `69bc535` APK-subject reader census from the native legs — 1 file, +5/-2
- `dd5cff7` native producer for the publisher's regression record — 1 file, +328/-13
- `1225caf` hosted-search stops spending an operation slot, lookup budget raised — 1 file, +20/-5
- `689994a` the core turn clock answers for the background lease — 5 files, +209/-53
- `48ad494` the owned User-Agent line plus the Accept line each route writes — 7 files, +311/-7
- `a2e3881` release-numbering stem — 2 files, +2915/-8
- `297937a` the 73-group registry pair and its twins — 7 files, +3669/-133
- `9cb116c` the 73-group connected record with both APK gates converted — 1 file, +118/-55
- `10fb71f` release-numbering and network-drive rows — 2 files, +382/-1
- `5f72682` clear the expected report before the run so a leftover is never credited — 1 file, +34
- `38add11` advertised tool set and active agent index in the history-migration twin — 1 file, +144/-11
- `dc1a411` the history-migration twin prints the retired leg byte-for-byte and clears its scratch on the abort path — 1 file, +29/-5
- `ab52052` dispatch a reference flag only to a group that names it in its own source — 1 file, +337/-6
- `8abd8fb` a refused candidate keeps its own refusal diagnostic; lookup mode keeps its own operation allowance — 1 file, +159/-45
- `76e637d` compare the same-build model-facing payload across compaction, not an invented ratio — 1 file, +150/-60
- `d86300a` each verification twin removes its own scratch tree on exit — 4 files, +54/-8
- `c57f756` the capture test reserves its scratch tree exclusively and removes it — 1 file, +54/-3 (HEAD)

Standing witnesses for these bytes, measured earlier in this session and **not** re-run to write this section:
a room of `dc1a411` reads `Rust cases passed: 895/895; isolated application cases: 61`; the live dirty tree at
`c57f756` reads `Rust cases passed: 942/951; isolated application cases: 61` with gate rc=1,
its nine reds pre-existing on uncommitted core bytes, and that run's gate stdout hashes to
`426f904380bd7b937068d2ef968f48b230c6f65c89ef849e48802b12bd828985`; a twins build of the tracked tree prints 74
`^twin:` rows. Nothing here re-litigates a settled leg.

Open lane state at the same moment, five candidates staged and none installed: `verification/history_migration.rs`
`2b5c987eb64f…`, the four-twin claim set `verification/{carry_flow,diagnostics,drive_ownership,turn_runtime}.rs`
`d10d08539c1e… / 8d1613ec6ef8… / dd535f71c9e6… / 96cdf1ae1f08…`, and the nesting-aware scanner candidate
`tools/harness.rs` `10c950d6bed3…`. Each live target is porcelain-clean and equal to its HEAD blob, so every one of
those installs would move bytes; each is held until an independent re-audit card whose subject digest equals the
bytes to be installed. Not measured for this section: the per-commit gate lines for the 20 product commits above.

### Repin: the record's own HEAD pin moved while this section was written (2026-11-17T09:45Z)

The section above pins HEAD at `c57f756`. That pin is now stale by one commit and is repinned here rather than
left in the file unqualified. `git rev-list --count 629683b..HEAD` = **46** commits, **21** of them touching
`core/`, `tools/`, `verification/` or `platform/`. HEAD is `7271ead7988da1f63027fec9cd879949230cbb15`
("Scope the scratch-cleanup claim in the history_migration twin to what its guard covers"), which carries
`verification/history_migration.rs` at `2b5c987eb64fbe6276e7966587ba1f328a188fb22f3a581847b453481fc8b1a5` —
the first of the five staged candidates listed above, now installed, so that row is closed and the remaining
four (`verification/{carry_flow,diagnostics,drive_ownership,turn_runtime}.rs`) plus `tools/harness.rs`
`10c950d6bed3…` are still staged-uninstalled. The tree digest is identical across `7271ead` and the two
superseded message rewrites of the same commit, which are now unreachable; no gate leg was re-run for a
message-only change.

One census correction, because the record cites this number: the count of commits carrying the attribution
trailer is **177** on `refs/heads/main` measured by the vendor token (`grep -c -i alibabacloud` over
`git log --format=%B`). A looser `grep -c -i 'Co-authored-by'` prints **178**, and the extra match is not a
trailer at all — it is the phrase inside the body sentence "Cleared: this window's commits carry no
Co-authored-by trailer (author Shawn Fowler)." of commit `31f24b9ff6ed0bca2d0ee2abcfc538410d1fd3ad`. A trailer
census must be anchored to the trailer position or it counts its own prose.

### Repin: the runner scan landed and four staged twins were superseded mid-lane (2026-11-17T11:01Z)

This document last moved at `8b01007`. `git rev-list --count 629683b..HEAD` = **48** commits, **22** of
them touching `core/`, `tools/`, `verification/` or `platform/`. HEAD is
`75756f0bb24a6b9786eac2c96b17c6cc4a72b391` ("Make the runner's source scan nesting-aware and
language-aware"), whose own shortstat is **1 file changed, 347 insertions(+), 73 deletions(-)**: it
installs the nesting-aware scanner candidate `tools/harness.rs` at
`10c950d6bed3fe91fa6db137c055f37e850d8fb83716fe6e4c8cfaf51fd08145` (179,375 B, 3,960 lines) over the
previously installed blob. The install ran only after an independent re-audit card whose subject digest
equalled those bytes and whose blocking-finding list was empty; the runner now scans nesting-aware block
comments and masks Python string/f-string bodies, and the language is threaded to the declaration probes
the scan feeds.

The four-twin claim set is still staged-uninstalled, but its keys moved and the older ones are now
stale. The independent re-audit of the first staged set returned **FAIL** on one blocking finding — a
twin doc-comment asserted its scratch tree goes away "the same way the other twins that clean up after
themselves do", a comparative claim the audit contradicted — and the repair round deleted that clause in
all four files. Staged digests are therefore
`verification/carry_flow.rs` `05175710ea3f681dd6074af6953af43f82119da8598e9894c506dce4e8ac9c50`,
`verification/diagnostics.rs` `e8e01cdc2dac023a5e6ca7099da07e281ac3d7bd42681f6e542452081503804c`,
`verification/drive_ownership.rs` `d26e760f1d93db4b3a4e3ce00e0eff1c4748762376b72decbebd0c6453e5a5bb`,
`verification/turn_runtime.rs` `e34a339d0d0c6a55999c5e7280151e27db2178c64540859414735d44319052c6` —
each exactly 39 bytes smaller than the superseded `d10d08539c1e… / 8d1613ec6ef8… / dd535f71c9e6… /
96cdf1ae1f08…`, each a uniform +5/-3 against its live target. Every live target is still equal to its
HEAD blob, so all four installs remain non-vacuous; each install waits on a fresh independent re-audit
keyed to the NEW digests, not to the ones this record first published.

One new lane is open beside it: a runner janitor leg for the twin scratch residue, staged at
`output/harnj1/tools/harness.rs` (185,902 B, 4,128 lines, `150ba6a6facdb342cb6e4e8775c416c6955d50ae691489a0fe9d363defd7fe00`)
against the installed 3,960-line base, with no card yet. It writes only that one path, so it is
path-disjoint from the four twins; the behaviour coupling still orders the installs twins first, runner
second.

Census correction the record should carry: measured over all **279** reachable commits on
`refs/heads/main`, the anchored attribution-trailer census (`^[[:space:]]*Co-authored-by:` over
`git log --format=%B`) and the vendor-token census agree at **177**; the earlier looser grep counted one
extra match that is prose inside a commit body, not a trailer. No commit made while writing this section
carried any attribution trailer.

Not measured for this section: no gate leg was re-run (this is a documentation-only commit), and the
per-commit gate lines for the product commits between `629683b` and HEAD are still unrecorded; the
settled 895/895, 936/951 and 942/951 legs were not re-run and their verdicts still describe the bytes
they were measured on.
