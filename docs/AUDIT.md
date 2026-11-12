# Linux host, packaging and verification audit (2026-11-10, current through 2026-11-13T03:3Z)

This section is the current audit. The sections below it describe earlier signed candidates and keep
their original counts and wording.

## What was measured

- Core unit suite through the sanctioned runner (`python3 tools/checks.py --all-units`, and since this
  window the native `output/tools/gate <units>`, which prints the identical line and writes byte-identical
  report bytes — re-witnessed at `ecbff24`: from one HEAD-built units binary, both printed
  `Rust cases passed: 763/763; isolated application cases: 55` with exit 0 and both produced
  `output/checks/full-units.json` sha256 `5a131e40e7315753dfec87c22608f52199477d426f4973112254eb9b082607aa`):
  **763/763 cases plus 55 isolated application cases, exit 0** on the current tree (the delivered
  69-group record is backed by 638/638, measured at `c34b170`), measured with the pinned
  `rustc 1.99.1` toolchain from `tooling/env.sh` (the 638 count is the tree at `c34b170`: 597 at `600e44d`,
  +10 candidate-pool cases in `f2a6ad8`, +4 self-build environment cases in `63bf5c5`, +5 self-build ordering
  cases in `a9c62c5`, +4 refine-workspace cases in `5b898cd`, +9 call-edge/cross-reference cases in `a2a976e`,
  +4 durable-successor cases in `12cdc6b`, +5 cross-reference cap-disclosure cases in `c34b170`; continued by
  +4 in `4758d28` → 642 and +7 in `5c93aed` → 645, then 649 re-measured in a room at `e4b7452` — that +4 was not
  re-derived per commit — and +1 in `98fb0d9` → 650, +8 in `e4118b1` → 658, +9 in `e7d13ec` → 667, +5 in
  `db16ffa` → 672, +14 in `436b281` → 686, +15 in `28b29d3` → 701, +7 in `2b041bc` → 708, +10 in `06d1f6b` → 718,
  +5 in `b6457b0` → 723, +6 in `f28e463` → 729, +6 in `0b1cf39` → 735, +2 in `f1051da` → 737 (re-measured directly on
  `f1051da` in a room that held none of the pending lanes' work), +16 in `51ce540` → 753, +4 in `966580b` → 757,
  +6 in `c396323` → 763; the sums were
  re-derived by counting added `#[test]` lines per commit with `git show`, which is why `3e5eda1` (+3) and
  `112e251` (+1) are absent from the chain — those are `tools/` package-test legs, not core unit cases; every delta was
  re-measured by the coordinator in a room that held only the lane's own file, and C4's room was
  `git archive f2a6ad8` plus its file, which is exactly the tree at `63bf5c5`. The earlier 517 tally was additionally reproduced on the
  drifted `rustc 1.95.0` build, `output/units` vs `output/units-pinned`, log `output/units-pinned.log`). A bare
  `rustc --test core/console.rs` from the
  repository root reports 503 passed / 14 failed; every one of those 14 is a runner gap, not a product
  defect — the fixtures use the relative scratch root `toolchain/check-workspace`, which the bare run
  never creates, and `capture_job::resource_checks` needs `CLIENT_CAPTURE_TEST_FONT` pointed at a real
  TrueType font. `tools/checks.py` now supplies that font itself instead of leaving the batch red.
- Connected groups through `tools/harness.rs`: **65 passed, 0 failed, 4 unavailable of 69 groups**,
  census clean, exit 0 (2026-11-12T10:1Z, after the four commits of that window; `verification/RESULTS.json`
  came out byte-identical to the committed record — sha256 `7250328b5385…` — so the source at that point
  reproduced the delivered record exactly). The runner
  reports a missing development input as `UNAVAILABLE` instead of charging it to the product, and a group
  whose own source guards an optional reference now runs with that comparison recorded as an unexercised
  leg instead of being blocked to a recorded `0`.
  **That equivalence is now stale and the drift is measured, not assumed.** `2c6b792` registered
  `self_rebuild`, so the runner declares **70** names while the delivered record still holds **69**, and the
  runner says so: `INVENTORY DRIFT: the delivered report counts 69 groups while the runner has 70`. Publication
  is blocked by a failing group, a census problem, or an unparseable count — **not** by groups being
  `UNAVAILABLE`: measured twice, a sweep with all 70 groups unavailable still wrote a record, and a sweep with 7
  unavailable wrote one too (see the 16:2Z block). Until the record is republished, its `pe_declarations` leg (18)
  also stays behind the 28 that group now runs.
- `verification/RESULTS.json` now carries the current census: **69 groups, 65 passed, 4,185 checks
  executed**, with the 4 unavailable groups recorded as 0 plus their missing inputs and a
  `zero_count_disclosure` line (earlier revisions of this document described the record as holding
  65 groups / 4,228 checks — the previously delivered candidate, before `state_sealing` and
  `desktop_runtime` were added — then 67 groups / 4,016 checks before `turn_background` was added,
  4,041 checks before that group gained its turn-lifetime checks, and 4,044 checks before
  `apk_alignment` gained its artifact-resolution checks; 65 passed / 4,142 followed the rebuild of
  `output/published-controller`). The 4,142 → 4,185 movement is attributed per group, not summed blind:
  `permission_modes` +27 (the new group), `prompt_reference` 0 → 52 (the optional-guard refcensus fix),
  `services` 69 → 72, against `apk_alignment` 11 → 0 and `turn_background` 28 → 0 from the off-host signed
  APK, and the group total moved 68 → 69 on `permission_modes` alone.
  **The record's Rust-case leg is stale and is a measured drift, not a pass:** `full_rust_suite` pins
  **513/513 cases + 55 isolated** with `binary_sha256 b621eccf4208…`, while the tree measures 763/763 + 55 at
  `c396323`. Both packagers write that leg — `tools/package.py:52` (`validation['full_rust_suite']=full_units`)
  and `tools/package.rs:1908` (`upsert(&mut validation, "full_rust_suite", full.clone())?`) — and a packaging
  run is blocked behind the signed-APK
  publication inputs (`apk['build_inputs']` digests are asserted per path, measured 130 match / 31 drift /
  1 absent), so refreshing it is the owner-gated `PUB-INPUT-DRIFT` leg, not a coordinator action. The group
  sweep itself never touches that leg, which is why a byte-identical sweep record is not a unit-count witness.
  The runner prints `INVENTORY DRIFT` whenever `GROUPS ∪ HELPERS ∪ OPTIONAL` stops matching `verification/`,
  so the record and the runner cannot silently disagree. Note that the runner compares the record it read
  *before* the run, so the run that grows the inventory itself prints one `INVENTORY DRIFT` line naming the
  old group count; that line is expected once per inventory change, not a product red.
  The Python group path keeps its own literal group list, which had already drifted in **order** from the Rust
  runner's `const GROUPS` (68 names, same set, diverging at index 65) while nothing compared the two lists to each
  other: `tools/package.py` compares its `current-regression.json` rows against `tools/checks.py`'s own list, which
  is circular. `tools/checks.py` now parses the runner's `const GROUPS` and asserts list equality (membership *and*
  order), so a group can no longer reach one instrument and not the other. Both arms were falsified before trusting
  the guard — dropping `turn_background` aborts naming `['turn_background']`, and an order-only swap with identical
  sets aborts too — and the restored file was digest-verified.

## The unavailable groups and their exact inputs

| Group | Missing input |
| --- | --- |
| binary_flow | `output/pre-branch-controller`, `output/pre-auth-controller` |
| history_migration | `output/previous-controller` |
| apk_alignment, turn_background | the signed APK `PUBLICATION-MANIFEST.json` delivers as a companion file |

These are reference builds from earlier deliveries. This checkout has no git history, so they cannot be
rebuilt from source here; each is reported rather than skipped silently. A group that declares such an input
optionally is not blocked by its absence: `prompt_reference` executes 52 checks and names
`--previous-controller` as an unexercised comparison leg (the byte-shrink comparison is the only leg it loses),
which is why it no longer appears in the table. `field_flow`, `flags_flow` and `product_flow` also left this list
when `output/published-controller` was rebuilt from the recovered published source (37 / 28 / 30 checks).

`apk_alignment` and `turn_background` are the two groups whose subject is the signed APK. They executed on
2026-11-10 against the companion delivery (11 and 28 checks), and the record now discloses them as unavailable
because those bytes (6,932,498 B, sha256 `86b7d741…`) are no longer on this host: every APK under `output/` is an
unsigned build candidate. Placing the owner's existing signed APK in the project root or an `output/` build dir,
or exporting `RLLM_CHECK_APK` at it, restores both without any new signing credential.

## Android packaging

- `res/desktop/` held generated desktop exports. `aapt2 compile --dir res` rejects non-resource
  directories, so **the APK could not be packaged at all** from this tree. The exports moved to
  `output/desktop-web/`; `res/` now contains only `drawable`, `drawable-nodpi`, `values` and `xml`.
- `platform/com/local/client/Usage.java` uses `android.app.usage.UsageEventsQuery`, an API 35 class.
  Compiling against `platforms/android-34` fails with three `cannot find symbol` errors, so the pinned
  platform is genuinely required. `platform-35_r02` and `build-tools_r35` were installed and verified
  against the sha256 pins in `docs/TOOLCHAIN.json` before use.
- `x86_64-linux-android` Rust standard library was missing; `package.sh` builds both ABIs and requires
  it. It is now installed.
- With those in place an unsigned Lab-variant APK builds end to end (`CLIENT_UNSIGNED=1`, 25 validated
  archive members, `Alignment payload validation: 25 exact members`).
- **Signing cannot be reproduced on this machine.** No keystore exists here (`*.p12` is ignored), and
  `package.sh` pins the expected certificate digest per variant, so a generated key is refused. The
  delivered `release/RLLM.apk` recorded in `release/NOTES.md` (6,932,498 bytes,
  `86b7d74156a11dc6ee6d70866bb480e30b75bac57c3400878c86b9f205921c3d`) is absent from this checkout.
- `.gitignore` carries exactly one APK exception, `!release/RLLM-Lab.apk`, while `package.sh` names its
  artifact `RLLM`. The exception never matches anything the packaging script produces.

## Background continuity on the phone (2026-11-10)

Owner-reported defect: taking the APK out of focus (answering a text) stops the task and the reply never
arrives; it must keep running whether or not the activity has focus. Ledger `BACKGROUND-CONTINUITY`.

Two mechanisms were measured in the shipped surface before any change. The rate clock's `ratePaused`
early-return sat **before** its own `postDelayed`, so the first paused tick ended the clock permanently;
and the manifest promoted only `.VoiceSession`, `.Gateway` and the isolated `.Execution` process, so an
ordinary model turn ran on the Activity's worker pool in a process the OS is free to reclaim once it is
cached. The transport itself was never aborted by pause (`networkActive` throws only on `closed`/token
change) and the result still committed to encrypted history — the loss was process lifetime plus progress
reporting, not the request.

The repair adds one foreground service and fixes the clock. `TurnService` (164 lines) is started from the
model-effect start path (`Activity.java:602`), calls `startForeground` with
`FOREGROUND_SERVICE_TYPE_DATA_SYNC` and holds a `PARTIAL_WAKE_LOCK` timed by the turn's lease
(`turnLease :730` clamps 30 s–600 s of `timeout_ms`, defaulting to 120 s — see the lease measurement
below, the model effect carries no `timeout_ms` so every model turn runs on that default); the rate clock
now re-arms outside the `ratePaused` gate (`:715`) and reports elapsed from the turn's own start instant
(`refreshRateClock :722`); a settled reply is published as a notification with a tap-through to its
conversation (`turnSettled :746` → `publishReply :770` → `TurnService.post`), the held reply being drained
by the permission-grant callback `:185` or by `onNewIntent :148`.

Verified with the pinned toolchain (`tooling/env.sh`), the lane's own re-runnable proof re-executed by the
coordinator — `bash /tmp/bgc-prove.sh /home/shawn/RLLM` → **exit 0, all assertions PASS**: `aapt2 compile`
0, `aapt2 link` 0, `javac --release 8 -cp $CLIENT_ANDROID/android.jar` 0 over **18 sources**, `d8` 0 and
`d8 --min-api 26` 0, and the service/permission set read back out of the linked APK:
`.TurnService foregroundServiceType=0x00000001`, `.VoiceSession 0x00000082` and `.Gateway 0x40000000`
unchanged, `.Execution isolatedProcess=true process=":execution"` unchanged, the special-use subtype
property present, **15** `uses-permission` (14 + `FOREGROUND_SERVICE_DATA_SYNC`) and **4** services
(3 + `.TurnService`); `dexdump` carries `Lcom/local/client/TurnService;` and the new Activity methods.
Three files changed, nothing deleted. Note the javac form: `-bootclasspath <android.jar>` fails this tree
(`Unable to find method metafactory`), `aapt2 link --java <gen>` + `javac --release 8 -cp` is what works.

Not proven here: that the owner's phone now delivers the reply while backgrounded — `DEVICE-ACCEPTANCE`
is blocked (no device attached), so the claim rests on the mechanism, not on observed behaviour. The
artifact side is now closed as far as this machine can close it: `CLIENT_UNSIGNED=1
CLIENT_VERSION_CODE=$(date +%s) bash package.sh` (after `source tooling/env.sh`) rebuilt the Lab APK end
to end — `output/2026-11-10__22-31-38Z/RLLM-unsigned.apk`, 6,944,777 bytes, `versionCode 1794349898`,
`PKG_EXIT=0`, `Alignment payload validation: 25 exact members` — and reading the shipped artifact back
with the pinned tools shows `com.local.client.TurnService foregroundServiceType=0x00000001` beside the
unchanged `VoiceSession 0x00000082`, `Execution isolatedProcess=true process=":execution"` and
`Gateway 0x40000000`, 15 permissions and 4 services, and a `classes.dex` with 224 class descriptors
including `Lcom/local/client/TurnService;` (members `begin`, `finish`, `post`, `notificationsAllowed`,
`onStartCommand`, `onTaskRemoved`, `stop`, plus the lease constants). `verification/apk_alignment.py` run
against that APK: **passed 8, checks 8**, and a serialized sweep with `RLLM_CHECK_APK` pointed at it
reproduced **61 pass, 0 fail, 6 unavailable of 67 groups, 4016 checks** with a byte-identical
`verification/RESULTS.json` (`2541279ea3996fe7…`) — the Android change moves no connected total, and the
record does not embed the subject APK identity, so the coverage is evidenced by the run inputs and log
(`output/checks-sweep-android.log`), not by the record bytes.

Because `apk_alignment` only validates payload alignment and identity — it does not discriminate the
Android surface change — the continuity contract now has its own group inside the sanctioned sweep:
`verification/turn_background.py` (**28 checks**), registered in `tools/harness.rs` `GROUPS`. It reads
`AndroidManifest.xml`, `Activity.java`, `TurnService.java` and the core's effect emitters, then reads the
service declaration, the permission and service counts and the dex method sets back out of the packaged
APK with `aapt2 dump xmltree` and `dexdump`. The serialized sweep that added it: **62 pass, 0 fail,
6 unavailable of 68 groups, 4,041 checks** (`output/checks-sweep-turnbg.log`, record
`ec624858df383fcb…`); the structural diff against the superseded record
(`/tmp/RESULTS-2238-before-turnbg.json`) shows exactly one changed top-level key, `connected`, whose only
delta is `turn_background: 25`. The group discriminates: pointed at
the pre-fix APK (`output/2026-11-10__16-00-43Z/aligned.apk`) it fails at *"the packaged application declares
the turn service that outlives focus"*, and in a scratch copy where the rate-clock re-arm is guarded by
`if (!ratePaused)` it fails at *"the rate clock re-arms itself while the app is backgrounded so elapsed time
keeps advancing"*. The compile-and-dex leg stays in `/tmp/bgc-prove.sh`; the group does not repeat it.

### Turn lifetime questions, adjudicated (2026-11-10, 23:12–23:19Z)

Four questions were put to a MiniMax audit seat (MM4) and came back with the coordinator after that seat
wedged at launch (1 transcript record, 0 assistant records, 0 tool calls). All four are answered from the
source with the pinned toolchain, and each answer is now a named check:

- **Can the promotion or the wake lock outlive the turn?** No, by two independent bounds. `finish()` is
  reached on every turn-ending path (`Activity.java:265` model result/failure, `:757` stopped, `:769`
  settled reply, `:1208` `onDestroy`), and even if it were not, the wake lock is acquired with an explicit
  timeout (`wake.acquire(lease)`) and the ticker calls `stopSelf()` at `elapsed>=lease`, whose
  `onDestroy` clears `active`, drops the queued callbacks, `stopForeground(STOP_FOREGROUND_REMOVE)` and
  releases the lock. `START_NOT_STICKY` on every return path means a killed service is not resurrected
  without a fresh turn.
- **Can the wake lock expire before the turn does (the reported symptom again)?** The lease and the
  socket deadline are the same window, which is what makes them safe together: `turnLease`
  (`Activity.java:730`) is `max(30000, min(600000, optLong("timeout_ms", 120000)))` and the transport
  deadline for the same effect (`:611`) is `started + max(5000, min(600000, optLong("timeout_ms", 120000)))`
  — one default, one cap. Check: *"the promotion window and the request transport window share one default
  and one cap so the wake lock can neither outlive nor strand the turn"*.
- **What actually sets that window?** Not the model request. `timeout_ms` is emitted by exactly one site in
  the core — `core/services.rs:182`, the service/job path whose `purpose` is a `service_request:<ts>`
  ticket — while the model effect (`core/application.rs:909`, and the hosted-search continuation at `:785`)
  carries no timeout key at all. So every model turn is promoted for the shared 120 s default, and the
  30 s–600 s clamp in `turnLease` never moves. This corrects an earlier sentence in this document that
  claimed the lease came from "the turn's own request timeout". If the core ever starts emitting
  `timeout_ms` on the model effect, the lease, the transport default and this check must move together —
  that is what the check pins, and it goes red the moment they diverge (mutation arm `timeout`, below).
  The same reading says a model request cannot run longer than 120 s end-to-end in the shipped Android
  surface, which is a product ceiling worth the owner's attention, not a lifetime bug.
- **Can two turns share one promotion?** Yes, and that is the design: `begin()` reuses the live instance
  (`if(service!=null&&service.active){service.extend(...);return;}`) and `onStartCommand` re-enters
  `extend()` when already active, so a chained model effect (harness retry, hosted-search continuation)
  rewrites the ticket, label, start instant and lease on the same promotion and re-acquires the lock for
  the new window rather than stacking a second one. `turnActive` is a single `volatile boolean`, so the
  Activity never runs two model turns at once.
- **Can a committed reply be lost between settle and delivery?** The reply is committed to encrypted
  history before any notification leg, so nothing is lost from the conversation. What can be lost is the
  *announcement*: `publishPendingReply` is gated by `TurnService.notificationsAllowed`, so on API 33+ with
  `POST_NOTIFICATIONS` denied the user gets no signal while backgrounded even though the reply landed; the
  drains are the permission-result callback (`code==56`) and `openRequestedConversation` on resume. That
  stays an acceptance caveat for the owner's device, not a code path that can be closed here.

Three mutation arms on scratch copies prove the three new checks discriminate — each fails naming itself,
and the real tree's digests are unchanged (`Activity.java 6f9852b0…`, `TurnService.java 0f3994bf…`,
`core/application.rs d529ab78…`): `transport` (default 120000→300000) fails
*"the promotion window and the request transport window share one default and one cap"*; `timeout` (a
`timeout_ms` added to the model effect) fails *"only the service path carries a per-request timeout"*;
`extend` (the reuse branch reduced to `return;`) fails *"a chained model effect re-extends the live
promotion instead of starting a second one"*. The republished sweep: **62 pass, 0 fail, 6 unavailable of
68 groups, 4,044 checks** (`output/checks-sweep-tblift.log`, record `a56728b93745171b…`), whose structural
diff against the superseded record (`/tmp/RESULTS-2318-before-tblift.json`) is exactly two leaves —
`turn_background 25 → 28` and `total_executed 4041 → 4044`.

Two instrument traps from the packaging leg: the Lab variant
asserts `CLIENT_VERSION_CODE` even when unsigned (`package.sh:44`, exit 1), and the packaged manifest
spells service names fully qualified while the `aapt2 link` form keeps `.TurnService`, so a matcher
written for one form reports a phantom absence against the other. Reviewer notes:
`TurnService.onTaskRemoved` calls `stopSelf()`, so swiping the app from Recents mid-turn still releases
the promotion; `dataSync` is subject to the platform's per-app budget, and a refused background start is
caught, leaving the turn running in-process.

## Toolchain drift against `docs/TOOLCHAIN.json`

| Component | Pinned | Installed here |
| --- | --- | --- |
| rustc | 1.99.1 | 1.95.0 |
| NDK | r28c (28.2.13676358) | 30.0.16248370 |
| platform / build-tools | 35 / r35 | 35 / 35.0.0 (installed this session, checksum-verified) |
| JDK | 17.0.21+9 | 21 and 8 |

The build succeeds on the drifted toolchain, but the pinned reproducibility claim is not met by this
machine, and the resulting APK is therefore a build-pipeline proof, not a delivery candidate.

## Other findings

- Dead-code inventory measured with the coordinator instrument `.qwen/tmp/loop/deadcode_census.py`
  (block-based warning/location pairing, all three phrasings — never used / never constructed / never
  read — rustc 1.95.0): **46 blocks / 58 items / 50 names / 24 files** before any wiring lane;
  **23 / 34 / 30 / 16** at the audited generation; **6 blocks / 17 items / 15 names / 4 files** in the
  settled tree once all three wiring lanes landed (`output/coordinator-warn-final.log`). Those three
  bases are different numbers: an
  earlier draft of this file said "49 dead-code warnings", which is the compiler's `warnings emitted`
  total, not a dead-item count. All 17 remaining items are obsolete with a cited live
  equivalent (0 defects); they need an owner disposition, not deletion.
- Running the core tests from the repository root writes scratch directories into the tracked `toolchain/`
  tree; 57 such pid-stamped directories were removed.
- `verification/README.md` documents only part of the group inventory, and `tools/checks.py` names 64
  groups against the runner's 67.

## Linux and Windows host runtime

`core/desktop.rs` was compiled but unreachable: no build entry, no verification group, no vault restore
at startup, no `media_upload` leg, no microphone binding. It now has:

- `desktop.sh`, mirroring `headless.sh`, as the declared build entry.
- `verification/desktop_runtime.py` (24 checks) plus a 23-leg runtime self-test.
- **Startup restores its own sealed vault** before the first render; a foreign key or a truncated vault
  fails closed instead of silently starting cold.
- A per-action stdin loop that re-seals the vault after every settled action.
- The request wire is taken from `wire_base64` when present, and a request that carries a stored file
  now streams it through the core's owned `media_upload` ticket.
- Microphone capture: `pw-record`, falling back to `arecord`, feeding the core's 20 ms frames; the
  recorded utterance is stored by the core and sent to the configured recognition connection. A
  deterministic `RLLM_VOICE_SOURCE` fixture keeps this leg reproducible on a headless machine.

## Linux/Windows surface in Dazzle (2026-11-10)

- RLLM's own Dazzle document is `gui/project.dzdoc` (project_id 3018100863). It is still a scaffold: a
  `Chat` page, one extra page created by a probe, and five shared-catalog components on `Chat` —
  `Rail Navigation`, `Status Indicator`, `Chat Panel`, `Message Composer`, `Metric Strip` (nodes 4–8;
  document generation 13 after two probe transactions were undone).
- Authoring works without touching the shared Dazzle project pointer: `dazzle-builder command gui
  <verb>` binds to the directory passed to it. `library-search components "<term>"` returns an `id` and
  an `index`; the follow-up verbs take the numeric index. `library-place components <index>` takes no
  parent and lands on the document's active page. Probe calls mutate — `command gui page-create` with
  no arguments created a page.
- **`property-set` cannot author node geometry, and its receipt proves nothing.** It returns `status:0`
  for an invented property name (an arbitrary `zzz` was accepted and persisted) and appends an override
  record (`origin:9`) while the node's own `layout` field keeps its authored value: `layout.h = 72`
  after `property-set 4 layout.h 900`. The override is **inert in the render** — `output/gui/rail-tall.png`
  shows the rail unchanged. Two such override records remain on node 4 at generation 17.
- **`preview` is frame-stable but captures Studio chrome.** Three renders with nothing changed between
  them gave byte-identical framebuffers (`5a73d7d6…`), so render differentials are a sound instrument —
  but a before/after framebuffer delta caused by a property write came from the Inspector panel, not the
  page. Geometry must be judged from a page-only render (`page-export`/`page-set`).
- **`patch`/`dry_run` op vocabulary is not guessable and is still unresolved:** both require
  `base_generation` plus an ops array; two plausible op shapes returned `unsupported patch op`, and
  `diff` does not echo the vocabulary.
- The `export-web` bundle in `output/desktop-web/desktop/` (`display_name=RLLM`, 9 widgets, document
  generation 51) came from a Dazzle document that no longer exists on disk, so it records the intended
  flow (`Page, Title, Draft, Status, Frame, Tabs, Messages, Composer, Send`) but is not a design source.
- **The document has no state substrate at all.** `export` of generation 18 (16,417 bytes, digest matching
  the receipt `8f65d665…`) counts nodes 8, bindings 0, tokens 0, actions 0, interactions 0, machines 0,
  models 0, variables 0, variable_collections 0, variable_modes 0, themes 0, locales 0; `active_page = 2`
  (Chat), page id 3 is an empty page named `Page`, canvas 1440×900, `page_target` empty. Each of the five
  widgets carries a `catalog.*` block and an `application.binding` *property*, but the document-level
  `bindings` array is empty, so those labels are inert and the projection is a static mockup.
- Outstanding: page fills/theme, page layout, the remaining destinations (History, Workspace, App
  settings, Agent settings, Diagnostics) taken from the destinations and fields `fn ui()` in
  `core/application.rs` already emits, real state bindings to those fields, `target-generate linux`,
  and a verification group for the emitted surface.

## Capture seam: one engine, shared bounds, and an instrument gap (2026-11-10, 23:27Z)

The leg the MM5 packet dropped — `core/capture_file.rs`'s `remote*` cluster against
`core/page_capture.rs`'s `render*` family — adjudicates as **no bounds divergence**, and the reason is
structural rather than comparative: `page_capture::render` (`:28`) has **zero** qualified external callers,
and its only callers are the in-module wrappers `render_inputs` (`:87`) and `render_resources` (`:93`), with
`render_with_loader` (`:90`) a one-line pass-through. Every cap therefore exists exactly once — 256 KiB HTML
(`:29`), viewport ≤ 2048 per side and ≤ 1 Mi-pixel (`:30`), 32 stylesheets (`:41`), 256 KiB aggregate CSS
(`:43`), 128 images (`:50`), 8 MiB aggregate image input (`:60`), 32 MiB decoded (`:61`), 65 536 glyphs /
1 024 unique glyphs / 1 048 576 outline points (`:74`) — so there is no second copy for a bound to drift
against.

The two *entry* paths (`capture_file::capture` `:43` local, `capture_file::remote_render` `:91` remote) share
the `validate` refusal set (`:36`), the 256 KiB input cap, the existing-output refusal (`:46` / `:93`, same
wording) and the whole save leg (`save_bytes` `:62`: `create_new` + `custom_flags 0x20000`, `write_verify`,
dev/ino identity recheck, two cancellation refusals). They differ only in budget-admission shape
(`access.budget` in two stages vs the job's `budget + reserved` test) and in who runs the page program: the
local path runs `page_program::run` itself (`:50`), while the remote family is handed already-classified HTML
by `web_document::capture_context_classified` (`core/web_document.rs:73`) and is re-admitted at the job
boundary with the same 256 KiB cap plus a second `capture_file::validate` (`core/capture_job.rs:25`). The
`remote: bool` fork selects only the base-URL **policy wording**: under `remote=false` a `<base>`-bearing
document's stylesheet/image nodes are noted and the loader is skipped (`:38`, `:59`); under `remote=true`
the loader runs and the inline path refuses everything (`capture_file.rs:76`). Nothing is fetched either
way, so the one defect candidate is a note that says “local capture” for a remote inline capture — cosmetic.

Projection loss is bounded too: `render_with_loader`'s loader signature drops the node id and
`ResourceKind`, which `capture_file` re-synthesises as `usize::MAX` / `ResourceKind::Png` (`:104`); measured
that no loader on that path uses node identity, so nothing is lost today — a latent constraint, not a defect.
No function in the cluster is dead: `render_inputs` has exactly one caller, and every entry point has a
production call site (`capture_file.rs:56`, `:103`; the `*_observed` variants at `capture_job.rs:84-85`; the
capture tool op at `application.rs:3631`). The block at `page_capture.rs:18-27` is a wiring note rather than
stale doc — it illustrates the loader-less signature as a dropped scoped refusal — but its cited line
`capture_file.rs:100` has drifted (the refusal is `:76`, the call site `:103`).

**The real finding is coverage.** `core/page_capture.rs` and `core/capture_file.rs` hold **0** `#[test]`
cases (`capture_job.rs` has 2), and no verification group asserts the capture refusals — “HTML capture input
exceeds 256 KiB”, “1 Mi-pixel” and “already exists; choose a new PNG” appear only in `core` sources, while
`verification/capture_lifecycle.py` covers the Android `AudioRecord` start/stop lifecycle, not page capture.
The entire capture bound set is therefore uninstrumented end to end. Owner's call which way to close it: a
69th connected group (a `verification/*.py` file is not a build input, so it cannot move `PUB-INPUT-DRIFT`)
or unit cases in `core/page_capture.rs` / `core/capture_file.rs` (both **are** build inputs, so they widen
the existing 26-input drift).

## Release artifact resolution: the instruments stop inventing a path (2026-11-10, 23:31–23:44Z)

`RELEASE-APK` had been reported as “pending on `release/RLLM.apk`”. That path is a phantom: the publication
record delivers the signed APK as a **companion** file, and `release/` holds only `NOTES.md`. The defect was
in the instruments, which invented a default path instead of reading the record:

- `verification/apk_alignment.py:11` and `verification/turn_background.py:34` each resolved the subject with
  `os.environ.get('RLLM_CHECK_APK', <release/RLLM.apk>)`.
- The runner gated the same phantom at `tools/harness.rs:132`: with no `RLLM_CHECK_APK` it probed
  `release/RLLM.apk`, so a companion delivery could never satisfy availability, and a group run directly would
  have opened a nonexistent file.

Both groups now carry a `subject()` that honours `RLLM_CHECK_APK` first, uses `release/RLLM.apk` only when the
record does not say `companion`, otherwise looks for the record's basename at the project root and under
`output/*/`, and refuses by naming the record. `tools/harness.rs` mirrors that resolution in
`fn published_apk` (`tools/harness.rs:146-182`), so a missing companion is reported as a missing input that
names the artifact. Three named checks pin it (`apk_alignment` 8 → 11 checks): the instrument set resolves its
subject from the record, the record names a delivered signed APK while `release/` claims none, and
`tools/readback.rs` (built by `tools/build-tools.sh` into `output/tools/readback` since the 2026-11-12
port; formerly `tools/readback.py`) verifies the companion by name and byte identity rather than by a
release path.
Runner side, measured with `RLLM_CHECK_APK` unset:
`apk_alignment UNAVAILABLE (not executed, missing: path companion APK RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk named by PUBLICATION-MANIFEST.json (not on this machine))`.

Proof that the checks discriminate (four arms, each red naming exactly its own check, then a clean PASS;
`/tmp/apk-mut2.sh`): re-introduce the hardcoded default in `turn_background.py`; shorten the manifest's
`sha256`; `touch release/stray.apk`; replace `readback.py`'s companion branch with `pass`. Every file was
restored by copy and the post-arm digests equal the baseline. Direct refusal proof — with the pinned env and no
`RLLM_CHECK_APK`, `python3 verification/turn_background.py` exits 1 with
“refusing to invent an artifact: PUBLICATION-MANIFEST.json delivers
`RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk` as a companion file that is not on this machine”.

Two instrument traps surfaced and are recorded so they do not recur: the first guard searched for the literal
`os.environ.get('RLLM_CHECK_APK',` inside the very file containing that pattern, so it failed on a clean tree
(the needle is now split as `"RLLM_CHECK_APK'" + ","`), and `git checkout --` silently no-ops in this
non-git tree, which leaked two mutations into the first arm run until the identical-red across all four arms
exposed it.

What is left on `RELEASE-APK` is owner-only: the companion triple and `output/checks/packaged.json`
(`tools/package.py:117`) are not on this machine, so the readback instrument has nothing to verify until a signed
package leg runs with `CLIENT_KEYSTORE`/`CLIENT_KEY_ALIAS`/`CLIENT_KEY_PASSWORD` or the bytes are placed here.
No keystore was minted. `verification/*.py` and `tools/harness.rs` are not build inputs, so the record's 162
pinned input digests did not move.

## Seam pairs adjudicated with coordinator instruments (2026-11-10, 23:39Z)

Four MiniMax audit seats in a row (MM2 mid-call ~65 min, MM3 16 min, MM4 13 min, MM5 19 min) produced **zero**
assistant records on this class of work, so the seats come off it and the pairs were measured directly. All
three are single-implementation seams — no divergent duplicate, none dead:

- `core/search.rs:221` `results` is the only source-response parser; `core/search.rs:249` `result_data` is a
  tuple projection that calls it, and its live caller is `core/application.rs:2427`.
- `core/css_selectors.rs:130` `matches_with_budget` refuses with `selector-match-work-bound` when
  `*work >= WORK` and otherwise delegates to `matches` (`core/css_selectors.rs:121`). The only production
  entry is the budgeted one (`core/css_cascade.rs:104`); the bare `matches` is reached from the wrapper and its
  own tests. The `.matches(` hits at `core/network.rs:304` and `core/stream.rs:725` are std `str` methods,
  not this type.
- `core/text_encoding.rs:107` `decode_classified` is a one-line pass-through to `decode_with_cancel` with an
  identical signature — despite the name it classifies nothing. The `Response`-vs-`Resource` typing lives in the
  caller: `core/web_document.rs:55` maps `text_encoding::TextError::Response` to `PageError::Response` and
  everything else to `PageError::Resource`; the gzip leg at `core/web_document.rs:50` does the same for
  `http_content_coding::ErrorKind`.

Instrument state: no `verification/*.py` names any of these files, so the only instrument over them is the core
unit suite (`core/search.rs:255-262`, `core/css_selectors.rs:138-144`, `core/text_encoding.rs:133`), green at
517/517 through the sanctioned runner. `decode_classified` could be inlined at `core/web_document.rs:55` with no
behaviour change — a rename decision, not a defect.

## The desktop projection builds here, and its layout gap is not RLLM's (2026-11-10T23:55Z–2026-11-11T00:05Z)

`dazzle-builder target-generate gui linux` ran against the authored document (generation 45) and returned
`status=0 bytes=321 sha256=b7d131e82ec1d3b0bf898a21275a4674f28de83a43a67ebb3ded2fe522c69449
requested_caps=851967 admitted_caps=851967 workers=1 pack_mode=runtime` with `identity: 28/28`,
`migration: 1/1`, `accessibility: 28/28`, `source-map: 0/0`. It wrote `gui/targets/linux/` — eleven projection
files plus `projection/{Cargo.toml,Cargo.lock,build.rs,src/main.rs (47,706 B),src/login_account.c (25,336 B)}`,
each with a sha256 in `index.csv`. `gui/` is not one of the 162 publication build inputs, so this write does not
move `PUB-INPUT-DRIFT`.

The projection was assumed to conflict with RLLM's build rule (pure standard-library Rust, no Cargo, no
third-party crates). Measured, that assumption is wrong: `Cargo.toml` declares zero dependencies, `src/main.rs`
imports only `std`, and the only foreign code is the one C file `build.rs` compiles. Reproducing `build.rs` by
hand with the pinned toolchain — `cc -c -O2 -std=c11 -fPIC src/login_account.c`, `ar rcs liblogin_account.a`,
then `$CLIENT_RUST/bin/rustc --edition 2021 -O src/main.rs -l static=login_account -l X11` — exits 0 with one
dead-code warning and produces a 4,689,976-byte ELF 64-bit PIE linked against `libX11`. The desktop projection is
buildable under RLLM's own rule; it needs libX11 and one C account module, nothing else.

Its runtime surface was then measured without a display. With `DISPLAY` unset it prints `display unavailable`
and exits 1. `--show` prints the control census — 27 objects under id high `5787775650370682881`, low ids
descending from 3018100862, exactly the six pages and 21 widgets in the document. `--activate <high> <low>`
prints `action unavailable` and exits 2 for every id tried (Chat, Rail Navigation, Chat Panel, Message Composer,
and an authored action id). The reason is in the projection itself: all 27 entries in `controls()` carry
`kind:1, gate:0, x:0,y:0,w:8,h:8`, so the authored geometry (Rail Navigation 240×900, Status Indicator h 72,
page layout) never reached the app, and `main.rs:887-890` refuses a `kind==1` control whose `gate != 2`. That
matches `manifest.txt` (`mapped=0/28`, `admitted_caps=0`, `toolchain_invoked=0`), `source_map.csv` (every row
`identity_preserved=0 mapped=0 content_preserved=0` even though the summary printed identity 28/28) and
header-only `tokens.csv` / `data_actions.csv`.

That gap was then discriminated instead of charged to RLLM. The owner's mature reference document
(`/home/shawn/Strata-data/GUI`, 64 nodes) was copied and projected with the same verb: `identity: 64/64`,
`accessibility: 64/64`, manifest `mapped=0/64 admitted_caps=0`, and a control table that is *also* uniformly
`x:0,y:0,w:8,h:8` with `gate:0`. The linux target emits stub geometry for every document, including the surface
the owner already ships, so this is Dazzle-side, not an RLLM authoring defect.

Substrate authoring was pinned on a scratch copy (`/tmp/gui-probe`), never the real document, because this CLI
applies immediately. The verbs split into two families. The positional family writes substrate that projects:
`action-define <name> [cancellable]` → `{"action_id":30,"status":0}`, `token_define <name> <type> <value>` →
`{"token_id":"32"}`; after those two, `inspect` showed actions 1 and tokens 1, and re-projecting added
`data,...,connector,...` + `action,...,refresh-status,...` to `data_actions.csv` and `accent,...,0xFF4EDFEA` to
`tokens.csv`. `interaction-set <source_id> <event> <action_id> …` prints its own usage line but refused
`invalid argument` for events `0`, `1`, `2`, `click`, `press`, `tap`, `pointer` — its event grammar is still
unpinned. The JSON-body family cannot author document state through this CLI at all: `data-configure` returned
`status=2 validate_status=2 record_count=0` yet wrote a node named `connector`, and `data-bind` returned
`status=2 record_count=0` for four body shapes while document `bindings` stayed 0. Dazzle's own audit card
explains why (`Dazzle/.cursor/state/coordinator-20260926T031844Z.jsonl`, quoting
`src/builder_runtime.cpp:11700-11847 run_data_surface_command`): the data path is "engine-per-call, no Document
integration, no surfaces generated" — it creates a `DazzleDwEngine` (11771), registers the connector (11788),
queries (11802) and destroys the engine (11815).

Finally, the consumer census: nothing in RLLM reads `gui/`, `gui/project.dzdoc` or `gui/targets/` (grep over
`core/*.rs`, `tools/*.py`, `package.sh`), and `core/desktop.rs:527-528` is `let (mut state, ui) = startup(...)`
followed by `println!("{}", ui.string())`, resuming from stdin at `:559-560`. Today's desktop host is a
JSON-lines host that renders nothing. Whether the desktop host should consume the projected X11 app or stay the
`ui()`-JSON host with Dazzle as authoring/format authority is an owner decision, recorded as such on the
DESKTOP-SURFACE row.

One more measurement retires a tempting next step: the authored substrate has **no consumer inside the projected
runtime**. Neither projection mentions interactions (`grep -ic interact` = 0 in both), and neither contains an
action or token table (`kAction|kToken|ACTION` = 0 in both) even though `data_actions.csv` and `tokens.csv` do
carry the authored rows. The whole interaction model is one generated function —
`fn activate(c,f){ if let Some(i)=c.field { match c.kind { 2=>f[i].checked_add(1), 3=>f[i]=if f[i]==0 {1}else{0}, _=>{} } } }`
— keyed on the projected control kind, so a document action, token or interaction is never consulted. With 27
`kind:1` controls in RLLM's projection and 62 `kind:1` plus a single `kind:2` in the owner's 64-node reference,
the generated app has one interactive control. Pinning `interaction-set`'s event grammar would buy RLLM nothing.

## Permission modes, per-connection limits and the web-search posture (2026-11-12T00:10Z–01:10Z)

The owner was being asked to approve operations every few seconds and asked for
explicit modes, a YOLO mode, and a per-connection override. `core/permissions.rs`
owns three harness modes: `review` (labelled **Ask**, and behaviourally exactly the
previous posture), `limited` (read-only operations run, anything that writes,
captures or uploads is reviewed) and `yolo` (no review dialog). A mode is only a
*stand-down* inside the existing `application::automatic_approval` re-entry: the
hard refusals in `harness_rules::check`, the must-ask rules in
`harness_rules::review_required`, `format_review` on recovered prose and an explicit
user deferral all still win. A mode can remove a review; it can never grant a
permission.

Each saved connection can be tighter than the harness mode.
`state.permissions["<connection index>"]` carries `mode`, `web` and `file` as
`inherit`/`ask`/`off`. It lives at the top level because `profile_save` replaces the
whole profile record with `provider::normalized(edit)`, and it resolves through
`harness::route(st,"primary"|"subagent")`, so a child task is bounded by the child
connection rather than by the chat connection. `ask` re-enters the review path;
`off` refuses at dispatch and names the connection
(`File access is off for Probe (App settings → Permissions)` — the manual search path
refuses before any request instead: `Web operation blocked before execution: …`).

Web search is treated as the tool it is: `web_search` and `web_read` stand down under
**every** mode, including Ask. Only a written harness rule, the app-wide
review-my-searches setting (`web.search_without_review`) or that connection's explicit
web `ask` can put a search in front of the owner; the mode menu states that in its own
description text. Native search stays the provider's job: the connection's
`hosted_search` choice is the authority, and with provider search on the request
declares the provider's own search while the harness `web_search` tool is removed from
the catalog and refused. New in this pass, for photo/video/voice-class capabilities: a
capability route with **no** saved default now resolves to the provider the
conversation already talks to (`services::provider_routes`, host authority of the
selected connection) instead of refusing; a third-party route without a saved default
is still not auto-selected, which preserves the voice-setup contract that a capability
whose default was removed is "not ready" — the first, wider form of that rule turned
`voice` red on exactly that pin and was narrowed.

Surfaces: the Android chat footer reads LLM · agent · permission mode · tok/s and the
mode menu is one tap from it; App settings → Permissions carries the harness mode plus
one mode/web/file dropdown per saved connection. Both modals are core `ui.modal`, so
the desktop host forwards the same action vocabulary with no desktop-specific mapping.

Measured: unit gate **532/532** Rust cases (`permissions` 10 cases, new `services`
capability-default 5 cases, 55 isolated application cases); the new connected group
`permission_modes` carries **27** owner-path checks against a real workspace and a
local search endpoint. Coordinator connected sweep 2026-11-12T01:37Z–01:41Z,
republished after the three new `services` legs grew that group to **72**: runner
holds **69** groups → **65 passed, 0 failed, 4 unavailable** (`binary_flow`,
`history_migration`, `apk_alignment`, `turn_background` — reference controllers and
the signed companion APK, all owner-gated), **4,185** checks executed
(`connected.counts['services']=72`, `connected.counts['permission_modes']=27`,
`zero_count_disclosure` unchanged); `verification/RESULTS.json` sha256
`7250328b5385`. The first sweep of this window went
red on exactly one `voice` check and the rule behind it was narrowed, as above. The
top-level `full_rust_suite` field (513) is carried forward by the runner and is not a
current run; the current sanctioned count is 532/532. The runner's own closing line
("recorded in verification/RESULTS.json: 69 groups, 4182 checks") reads the record that
was on disk when the sweep started, so it understates the run that just published it;
the published witness is the earlier `delivered record written: 69 groups, 4185 checks`
line plus the file digest above.

## Still open

### Recursion and reverse-engineering gaps (2026-11-11 report, adopted 2026-11-12)

`docs/RLLM-recursion-gap-report.md` (verdict **BAR-MISS**, static evidence) is now part of the
open-items list, as ledger obligations `GAP1`-`GAP5`, ranked as the report ranks them:

1. **`GAP1-SELF-REBUILD-RELOAD`** — nothing rebuilds RLLM and switches the running work onto the
   new bytes. Verified: no `Command::new` in `core/` spawns a build (`core/desktop.rs:214,267`,
   `core/execution.rs:93` are the only spawns); no core or runner file references `package.sh`,
   `build-units.sh`, `build-harness.sh`, `headless.sh` or `desktop.sh` at all
   (`tools/harness.rs:955` is a descriptive string, not a call). Done bar is a **run log**:
   edit → build → test → revision → re-exec/successor → same job continuing.
2. **`GAP2-RRSI-LOOP`** — the refine loop rewrites agent *instructions* only
   (`core/harness.rs:396-434`), was capped at **two** rounds (`core/improvement.rs:109-115`; raised to **three**
   with a production `round_index` writer and a `held_out` round in `f1051da`),
   scores only user phrases / JSON fields (`:36-67`) and keeps a candidate on a raw passed-count
   gain (`:120-124`). Missing: edit budget, edit history, stall exploration, leakage critic, noise
   floor, cost-aware acceptance, pruning, held-out/OOD split, parallel candidates, worktree
   candidates, resumable JSONL run with a STOP file.
3. **`GAP3-HARNESS2-TWO-LEVEL-RECURSION`** — PPC and the lessons store exist; no candidate pool,
   no composition from a pool, and no persistent improvement library that a later task reads back.
4. **`GAP4-REA-FEATURE-REBUILD`** — `reconstruct` copies bytes, exports a retained representation,
   or writes a structural spec; its own review warning says it "does not recreate original source
   bytes or behavior". Of the three named missing analyses, JS/Electron and .NET/CIL have landed
   (the .NET parser with three named caps and a live `inspect` consumer in `51ce540`); saved
   HAR traffic is read and wired (`core/har_evidence.rs` → `core/observation_records.rs:115`), and
   its **SAZ** container form is still unrecognised — there is no ZIP path in that reader.
5. **`GAP5-NATIVE-TOOLING-NO-PYTHON`** — measured 2026-11-12: **79 Python files / 8,004 lines**
   (`tools/` 8 files / 444 lines, `verification/` 71 files / 7,560 lines). The report names only
   the `tools/` eight; the exposure is larger, and this window made it worse because the new
   `permission_modes` group (27 checks) and the three new `services` legs were written in Python.
   `tools/harness.rs` is the only native instrument. Owner rule: RLLM does not run on Python where
   a native route exists — this is an open violation, not a neutral fact.

Substrate for `GAP1` step 4 ("commits it") and `GAP2` feature 12 (worktree candidates) was
resolved by the owner on 2026-11-12 (**both** a git substrate and a native content-addressed
journal). Measured: `/home/shawn/RLLM` is now a git repository — branch `main`, baseline commit
`727a33d`, 339 tracked files, clean `git status`, git 2.43.0. `.gitignore` gained `tooling/`
(5.6 GB pinned toolchain, restored by `tools/toolchain.py`, so it must not be tracked),
`.qwen/tmp/` and `.playwright-mcp/`; the pre-existing `!release/RLLM-Lab.apk` exception was left
alone, so `GITIGNORE-ARTIFACT-NAMING` is still an open owner decision. Initialising the repo does
**not** recover revisions 1793395702 / 1793592809 — history starts at the baseline, so the
reference-controller groups are unaffected.

**Progress on `GAP1` and `GAP5` measured 2026-11-12T02:50Z.** `GAP1` step 1 landed at commit
`2f44536`: `core/self_build.rs` (fingerprint / plan / journal / handoff plus the tool seam), four
`self_*` verbs in `core/lib.rs`, `self_build` in the `const TOOLS` catalog and operation reference
(`core/harness.rs`), approval routed through the write posture (`core/application.rs:3100`,
`harness_rules::check` with tool `write`, `permission_subject` feeding
`harness_rules::standing`/`permissions::stands_down`), and `bounded_process`/`exec_replace`
exported from `core/execution.rs` reusing that module's existing process-group cancellation. Child
processes are allowlisted to `bash headless.sh`, `bash tools/build-units.sh <units>` and
`python3 tools/checks.py --all-units <units>` with 900 s / 300 s walls
(`core/self_build.rs:29-30`); builders and `exec` refuse unless `review_confirmed` is set.
A **real run log** on the committed bytes is in `docs/SELF-BUILD-RUNLOG.md`: the product ran the
whole sanctioned plan, appended journal revision `d4fdee92…` with gate 538/538 and the pre-edit
binary as the rollback digest, detected a stale claimed digest, refused reload without review, then
`exec`'d the rebuilt image and the **successor** answered a verb that exists only in the rebuilt
source and appended journal revision `1c8ff4e0…` with parent `d4fdee92…` on the same pipe. The two
defects that came out of that log are **closed at 2026-11-12T07:4Z** (`1598752`): `self_build::run` now
calls `apply_reviewed_edit` before any builder leg and every receipt names the write
(`Saved N bytes to <path>.`), using the same replacement policy as `write` — regular non-symlink
target, `approved_previous` re-compared against the file on disk, bytes written to a fresh
`.pending-<pid>-<nanos>` temp file that is `sync_all`'d, then `fs::rename`; and `handoff` accepts
`journal`/`rollback` and walks with `tree(&root, &skip_paths(value)?)`. The reload fix is witnessed in
the product, not only by a unit case: in a clean room at `1598752` whose controller was built in the
room (11,983,664 bytes), `self_handoff` refused with `file exceeds 4194304 bytes` while an
11,987,760-byte rollback store sat at `self/rollback`, and the same call naming `journal` and
`rollback` proceeded; the journal commit itself succeeded there (revision `8c61e052…`, gate
**541/541**, isolated 55, rollback preserved) and refused a wrong parent before spending a rebuild.
The edit leg is now witnessed **through the product's own approval path** (2026-11-12T07:3Z, a room
extracted from `0f36aa3`): a model turn against a stub provider proposed `self_build`, the product
staged it (`dialog=approval`, `pending.tool=self_build`, `args.previous_exists=true` stamped by
`inquiry::prepare` at `core/application.rs:3822`), the owner `approve` dispatched it at `:3640`, and
the receipt read `Saved 21827 bytes to core/lib.rs.` followed by `bash headless.sh`'s own stdout —
the product wrote the reviewed bytes and then ran the sanctioned builder. The pair discriminates: the
pre-edit binary copy still answers `self_echo` with `Unknown operation` while the rebuilt binary
answers `{"canary":"successor-build"}`, and `builder="sh"` in the same proposal is refused with
`builder is not one of the sanctioned commands` (`docs/SELF-BUILD-RUNLOG.md`). What is **still**
missing is narrower than before: `self_build::run` (`core/self_build.rs:1005-1056`) has no
journal-commit arm — its arms are edit, `builder`, `gate`, `reload` — so gap-report step 3
(record the revision) is reachable only through the `self_journal` headless op, and no single
model-driven job has yet run edit → build → gate → journal → reload → continue.
`GAP1` also remains unproven in product reachability:
`self_build` is in `const TOOLS` and therefore in `agent_new`'s default tool string
(`core/harness.rs:166`), but it is absent from `REFINEMENT_TOOLS` (`:274`) and **no connected group
exercises it**. `GAP5`: `tools/alignment.py`, `tools/readback.py`, `tools/checkpoint.py` are deleted
and replaced by `tools/alignment.rs` / `readback.rs` / `checkpoint.rs` (3,568 lines, 8 `#[test]`
cases) built by `tools/build-tools.sh`, which ran green; tracked Python is now **76 files / 7,932
lines** (`tools/` 5, `verification/` 71) — three fewer files, 72 fewer lines. The four defects measured at 02:52Z are **closed and verified by the coordinator at 2026-11-12T06:44Z**:
`tools/apk.py:10` and `tools/package.py:105` name `tools/alignment.rs` (both literals re-read),
`package.sh` builds the tools inside the same input-drift window its fingerprint check measures and
then runs `output/tools/alignment` (`bash -n` clean), and `bash tools/build-tools.sh` compiles each
tool twice and executes its `--test` binary, printing `tool tests: … cases: 2`, `1`, `5` — 8 executed
cases, each equal to that source's declared `#[test]` count, refusing otherwise. The connected
instrument now drives the port: `./output/harness apk_alignment` in only-mode, with `RLLM_CHECK_APK`
set to that same 6,944,777-byte APK, printed `apk_alignment PASS (exit 0, 7s, 11 checks)` while
shelling `output/tools/alignment` for align and verify; the truncated, changed, missing, extra,
duplicate and CRC-corruption arms still refuse and the group's readback assertions match the Rust
source text. That subject is the packaging leg's unsigned `package.apk`, not the delivered signed
companion, so the published record's `apk_alignment` UNAVAILABLE arm remains open until the owner's
companion triple lands; `checkpoint.rs` still has no code consumer anywhere in the tree, so it has no
behavioural witness beyond its five executed unit cases, and `readback.rs` still needs that companion
triple. A real `package.sh` run at 2026-11-12T05:31Z (`output/2026-11-12__05-31-35Z`,
`CLIENT_UNSIGNED=1`) proves the native aligner inside the actual pipeline rather than only in a
standalone differential: that leg's `build-inputs.sha256` names `tools/alignment.rs`
(`f05de95d…`) among its 167 recorded inputs, and that inventory still equals the current expected
set, so `tools/apk.py:10`'s `'Build input inventory changed after compilation'` assert passes on
real bytes; two recorded per-file digests now mismatch (`core/execution.rs`, `core/self_build.rs`)
because the `GAP1` lane edited `core/` after that leg — the drift check working as designed. No
signed APK exists anywhere under `output/` or `release/` on this host, so the positive signature
path and the full `apk.py` inspection remain unrunnable here.
The port also moved the aligner from an imported source file to a **built artifact**, which is a new
way for a connected group to fail for a reason that is not the product: `verification/apk_alignment.py`
now shells `output/tools/alignment`, so a sweep on a tree where `tools/build-tools.sh` has not run
would have charged that group as a FAIL. Measured 2026-11-12T06:58Z with a private coordinator build
(`bash tools/build-harness.sh output/harness-verify`, leaving the shared `output/harness` and the
delivered record untouched): with the tool present the group printed
`apk_alignment PASS (exit 0, 6s, 11 checks)` against the 05:31Z leg's `package.apk`; with
`output/tools/alignment` moved aside it printed
`apk_alignment UNAVAILABLE (not executed, missing: path output/tools/alignment (built by
tools/build-tools.sh))`, so a missing build artifact is now classified as an unavailable development
input rather than a product red. `tools/checks.py` has no unavailable concept at all — every group
exit code must be zero — so the sanctioned `python3 tools/checks.py` sweep still requires the tools
to be built first, which docs/BUILD.md now states; and
**the aligner is proven equivalent on real bytes** — measured 2026-11-12T01:20Z against the
2026-11-11 packaging leg (`output/2026-11-11__14-59-03Z/package.apk`, 6,944,777 bytes), the native
`output/tools/alignment` printed `Alignment payload validation: 25 exact members; attempt 1` and
produced an APK **byte-identical** (`sha256 76cd2b996c6f…`, `cmp` clean) to the `aligned.apk` the
deleted Python aligner produced at that leg; verify mode returned 25 with rc 0, truncating the
candidate to 6,000,000 bytes refused with `BadZipFile: File is not a zip file` (rc 1), flipping one
byte at offset 4096 refused with `zlib.error: Compressed back-reference outside member bounds`
(rc 1), and the pristine control stayed green; and `GAP1` newly made the **product** depend
on Python — `core/self_build.rs:435,556,983` name `python3` where `core/harness.rs` and
`core/application.rs` named it zero times at the baseline.

**The `GAP5` verification boundary** — coordinator statement, measured 2026-11-12T08:3Z, owner-reviewable.
The report's bar is *"packaging and verification are native, like the runtime. A bridge is allowed only
where no native route exists at all, and the work must state why."* So the boundary was measured, not
asserted. Census over the **71 group bodies** (`verification/*.py`, 7,560 lines) by the external
mechanism each one actually touches: `subprocess.Popen` driving the product over a JSONL pipe **63**;
base64 HTTP-wire injection **55**; `http.server` stub peers **26**; `select` **36**; `threading` **32**;
`hashlib` **24**; `struct` (binary packing) **34**; `tempfile` **70**; `urllib` **7**; `zipfile` **3**;
`ssl` **1**; `png` as bytes **5**; `aapt`/`zipalign` **1 each**; the JDK/`CLIENT_TOOLS` binaries **6**/
**2**; `PIL` **0**; `apksigner` by name **0**.

Every one of those has a native route, and in several cases the route is already in this tree: the
product itself encodes and decodes base64 natively (`core/network.rs:193 b64`, `:210 unbase64`);
`tools/alignment.rs` and `tools/apk.rs` already parse zip members natively; `std::net::TcpListener`
replaces `http.server`; `std::thread` and `File`/`create_dir_all` replace `threading`/`tempfile`; and
the coordinator that *runs* the groups (`tools/harness.rs`) is already native — it simply shells
`python3 verification/<name>.py`. The JDK, `aapt`, `zipalign` and `apksigner` references are external
**binaries**, which the R-rule does not forbid; what is forbidden is the Python body driving them.

Therefore the honest boundary is **not** an exemption list, because there is nothing in that census for
which no native route exists. It is a migration order, with one shape worth naming because it avoids 71
separate build products: a single native verification binary with one module per group, dispatched by
group name, so `tools/harness.rs`'s command changes from `python3 verification/<name>.py` to that
binary. The interlock must be re-pinned in the same change — `verification/<name>.py`,
`const GROUPS` in `tools/harness.rs`, and the module-level `names` in `tools/checks.py` are 69/69/69
today and the `runner==names` assert exists precisely to catch drift — and once the bodies are native,
`tools/checks.py`'s group-sweep role becomes redundant with the native coordinator and should be
**deleted, not ported**, while its `--all-units` gate role is being ported now (dispatched as
`tools/gate.rs`). Two facts to keep honest about the cost and the limits: the remaining Python is
7,560 lines in `verification/`, which is the single largest R-rule item left in the tree, and the rows
that cannot execute on this host (`DEVICE-ACCEPTANCE`, the owner's signed companion triple, the two
unrecoverable controller revisions for `binary_flow`/`history_migration`) are unavailable **subjects**,
i.e. input gates, not Python dependencies — porting them changes nothing until those inputs exist.

Settled by the owner and not reopened by the report: no turn or round cap, YOLO never asks, web
search and fetch always approved in every mode, shell/git/hosted search fine as tools, and no
copy of `rrsi`, `harness2` or `rea` anywhere in the tree (re-verified: no such file or directory).

`output/previous-controller` and `output/pre-branch-controller` (2 groups — `binary_flow`,
`history_migration` — require built binaries of revisions 1793395702 and 1793592809, this
checkout only became a git repository at the 2026-11-12 baseline `727a33d` — whose history starts
there, so those two revisions are still absent — and a host-wide source census found exactly one out-of-project RLLM tree,
so those two revisions are not recoverable here; `prompt_reference` declares the same reference optionally and
now executes with its comparison leg recorded as unexercised), APK signing
identity, the missing delivered companion APK, the **3 dead-code warning blocks / 6 items / 6 names /
2 files** still reported by the pinned compiler (re-measured 2026-11-12T01:47Z with
`.qwen/tmp/loop/deadcode_census.py` over a fresh `bash tools/build-units.sh output/units` log —
`core/resource_url.rs:24` method `scheme`/`host`/`port`, `core/resource_plan.rs:6` `id`,
`core/resource_plan.rs:7` fields `node`/`target_span`; 11 warnings emitted in total; down from
46 blocks / 58 items before any wiring lane) — all
obsolete with a cited live equivalent, 0 defects, so they need an owner disposition, not
deletion — the `.gitignore` name mismatch, **background continuity on the phone** (implemented,
instrument-verified, and packaged into a rebuilt unsigned Lab APK whose `classes.dex` and manifest carry
the new service — still not observed on the owner's device, which is the only remaining unknown — ledger
`BACKGROUND-CONTINUITY`), and physical-device acceptance. The Linux/Windows surface is now authored in
Dazzle (six destinations, 28 nodes, 21 catalog widgets) and its `linux` projection builds here under the
pinned toolchain with no Cargo, but two things remain: whether the desktop host consumes that projected X11
app or stays the `ui()`-JSON host `core/desktop.rs` is today (owner decision — nothing in RLLM reads `gui/`
or `gui/targets/`), and the projection's stub geometry (`mapped=0/N`, every control `x:0,y:0,w:8,h:8`,
every `--activate` refused), which is Dazzle-side — the owner's own reference document projects the same
way. Live document state bindings still cannot be authored through the builder CLI at all (the data path is
engine-per-call with no Document integration). The five `Atomic::fetch_update` deprecations the pinned compiler reports are adjudicated as name
hygiene, not defects: a probe compiled and run under rustc 1.99.1 (`.qwen/tmp/loop/fetch_update_probe.rs`)
shows `fetch_update` and `try_update` are outcome-identical and that the failure arm fires only at
`u64::MAX`, where each of the five sites returns its own typed exhaustion refusal — renaming them is the
owner's call. The group-set consumers agree by name and by count, re-measured 2026-11-12T01:47Z:
`tools/harness.rs` `const GROUPS` enumerates **69** groups, `tools/checks.py` module-level `names` holds **69**,
and `tools/package.py` derives its expected set by AST-parsing `checks.py` while its `pairs` map (35 written
pairs + 22 + 12 appended) holds the same 69 names (the earlier "52 groups / 15 missing", "65 of 68" and
"68 groups" figures were all stale; the two hand-maintained inventories were widened to close the gap). The three groups that had been missing from those inventories —
`desktop_runtime`, `state_sealing`, `turn_background` — now carry **24 + 16 + 0** checks in the delivered
record: `turn_background` is recorded as 0 (not a pass) because its subject is the companion APK named by
`PUBLICATION-MANIFEST.json`, which is not on this machine; the other two execute, and all three reach the
packaging inventory; what still blocks the packaging path is the signed APK.
They are not free additions: `turn_background` resolves its APK subject from `PUBLICATION-MANIFEST.json` and
exits non-zero when that companion APK is not on the machine, which the Rust runner turns into an
`UNAVAILABLE` row but `checks.py` would count as a failure; `desktop_runtime` re-builds the host through
`desktop.sh` so it requires `CLIENT_RUST`; `state_sealing` needs no inputs at all.

`output/published-controller` is no longer missing. The published candidate's source tree exists on this host at
`/home/shawn/Dazzle/bloat/scratch/RLLM-CANDIDATE-20261109/RLLM` (`PUBLICATION-MANIFEST.json` version 1794270237),
and it verifies against that manifest's own pins — **162/162** `build_inputs` and **281/281** `/files` entries
match, zero paths missing. Building that tree's `core/console.rs` with the pinned host toolchain produced
`output/published-controller` (11,809,824 B, sha256 `97a42c14bd73…`), after which `field_flow`, `flags_flow` and
`product_flow` execute **37 / 28 / 30** checks; the connected census in `verification/RESULTS.json` now reads
**64 passed / 0 failed / 4 unavailable of 68 groups, 4,155 executed checks**. The four unavailable groups are
`binary_flow` and `history_migration` (the reference controller revisions they open are not on this host) and
`apk_alignment` and `turn_background` (the signed companion APK named by `PUBLICATION-MANIFEST.json`,
sha256 `86b7d741…`, is off this machine; every local APK is an unsigned build candidate). `prompt_reference`
is no longer among them: the runner now derives an input's requirement from the group's own source, so a group
that guards its reference runs and reports the lost comparison as an unexercised leg instead of being blocked
to a recorded `0` (`tools/harness.rs` `flag_required`, and `tools/checks.py` passes that reference only when it
exists). The provenance record the
publication path requires is written at `output/checks/published-comparator.json` (`source_path`,
`source_sha256`, `controller_sha256`), which `tools/package.py:47-49` re-hashes on every run.

Two Android turn-window facts the owner has not decided on, both measured this session: a model request in
the shipped surface is bounded at **120 s wall clock** (`timeout_ms` is never emitted on the model effect, so
`turnLease` and the socket deadline both take their 120000 default and `Activity.java:615` force-closes the
socket at that deadline), and with `POST_NOTIFICATIONS` denied on API 33+ a backgrounded reply is committed
to history but never announced (`publishPendingReply` is gated on `TurnService.notificationsAllowed`). The
first is a provider-latency ceiling, the second is the same user-visible symptom the fix targets; neither is
a defect the instrument calls red, and both are product decisions rather than code I should change
unasked.

Measured against `PUBLICATION-MANIFEST.json` (build 1794270237): `signed_apk.location` is `companion`,
so the source bundle deliberately carries no APK — `tools/readback.rs` refuses when the release
directory holds no APK list entry in that branch (`apks.is_empty()`, since the 2026-11-12 port of
`tools/readback.py`) —
and the companion bytes (`RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk`, 6,932,498 bytes,
`86b7d741…`) exist nowhere on this machine, which is also why the readback leg cannot be run here
(`output/checks/packaged.json` is absent too). Re-measured 2026-11-12T02:58Z against the 162 pinned build
inputs: 131 match the working tree exactly and 30
have drifted (`AndroidManifest.xml`, `package.sh`, `platform/com/local/client/Activity.java` and 27
`core/*.rs` files —
`application`, `capture_file`, `claim_records`, `comparison`, `control_flow`, `css_cascade`,
`css_selectors`, `delegation`, `dex`, `dex_code`, `directory_observation`, `elf_observation`,
`execution`, `harness`, `harness_rules`, `hosted_search`, `image_png`, `instruction`, `investigation`,
`lib`, `page_capture`, `page_layout`, `pe_observation`, `search`, `services`, `text_encoding`,
`web_document`). **One recorded input is now missing from the tree**: `tools/alignment.py`, which the
`GAP5` port deleted while `PUBLICATION-MANIFEST.json` still pins it (in `build_inputs`, in `files`, and
in `verification/RESULTS.json`), so `PUB-INPUT-DRIFT` is no longer only stale digests — the manifest
names an input that does not exist.
Six files the publisher would now enumerate are not in the manifest at all: `core/desktop.rs`,
`core/permissions.rs`, `core/self_build.rs`, `core/state_crypto.rs`,
`platform/com/local/client/TurnService.java` and `tools/alignment.rs`. So `tools/package.py:106`
("Source differs from inspected APK inputs") and the per-input digest assert at `tools/package.py:11`
refuse both a republish and the readback leg until a publication is made from the current bytes — which
also needs the signing identity this machine does not have. The earlier figure for this same check was 7
of 162; it grew because the wiring lanes and the Android fix landed, and nothing was reverted to shrink
it.

---

# Current audit scope

An installed-app report exposed valid-looking request JSON appended to useful commentary. The unchanged signed1793982425 controller reproduced the generic warning replacing that text and offering no direct review path. Explicit single-request review now preserves text and current identity/permissions, requires a separate one-time approval, and cannot use matching saved approvals. Four-format checks cover examples/ambiguity/hidden reasoning, changed connections, actual matching session rules, actual local HTTP and model continuation. Alternative-source search is an explicit exact-query review with one chosen endpoint; custom endpoint/credential isolation is retained. Current-byte results and device/independent limits are recorded below and in RESULTS.json; no current weather or private-server behavior is inferred from unrelated search results.

The current signed candidate, executed counts and acceptance limits are recorded in [RESULTS.json](../verification/RESULTS.json), [STATUS.md](STATUS.md) and [release notes](../release/NOTES.md). The dated checkpoints in project-progress.md preserve actual before/after observations. Owner checks are not independent or physical-device acceptance.

The audit records below describe their original source versions and retain their historical counts and wording. Their uses of “current” refer to those original versions. Later foreground spoken interruption, audio-route/focus ownership, bounded scene workers and voice-review lifecycle repairs are described by the current files above. Older broad scope remains preserved.

The connected voice-admission review reproduced a late resource refusal that removed the speech request but retained Preparing speech. Scoped service interruption clears only its matching voice phase and pauses that loop. Actual PCM/HTTP/public-event checks cover refused recognition and synthesis, retained primary answers, explicit fresh resume and sufficient-memory playback. Candidate1793981376 also verified isolated service cleanup; a later overlap probe found that a completed microphone utterance could replace a separate pending or running service request.

The connected ownership repair pauses capture when another proposal, operation or task starts and checks admission again at utterance completion. Real completed PCM receipts held across separate request review and dispatch must leave those states exact, with no new network work. Separate HTTP results still complete, and explicit resume creates a fresh microphone afterward. Service refusal retains the already-paused voice session rather than rearming it. These are owner checks using controlled platform/model responses; installed Android and actual user-service acceptance remain unverified.

Adjacent speech-preparation failures reproduced stale Preparing reply after an over-bound answer and stale Preparing speech at the actual 512-file media quota. The scoped answer-failure path now pauses voice and retains the full primary text and specific refusal explanation. These cases do not truncate text, send another request or automatically restart capture.

---

# Historical audit records

# Foreground open-microphone update

The user extended the previous manual voice scope to optional automatic conversation turns. Rust now segments direct microphone PCM and owns expiring capture, recognition, model, synthesis and playback transitions. Android bindings provide actual microphone permission/capture and decoder callbacks. Capture stops before recognition and remains stopped through playback. No platform speech recognizer or bundled neural transcription model is required; recognition and synthesis are independently configurable server capabilities.

The current voice checks passed126 owner-executed cases across all four model formats, including actual local HTTP audio transfers, automatic next turns, quiet/noise suppression, stale/duplicate ownership, stopped/failed playback, foreground suspension, uncertain request failure and a missing recognition route that leaves primary text usable. These use controlled PCM, model replies and playback callbacks; physical Android hardware and live user endpoints are not certified. Broader fresh results are recorded in verification/RESULTS.json when the matching signed package is produced.

The prior candidate audit below retains its original execution, counts and observations. It is not fresh execution for this update.

# Optional voice and media upgrade audit

This section describes the current signed candidate. The older audit below retains its original baseline scope and counts; it is not fresh execution for this upgrade.

The current source passes 34 of35 connected groups and194 selected Rust cases. The binary-flow group records94 current checks before stopping at3 unavailable revision-two compatibility comparisons. Its historical reader fingerprint was not weakened. Fresh complete serial Rust sweeps of this source and the exact preserved baseline each report454pass/14fail across468cases, with identical failing fixture names. Raw current/baseline logs and the comparison are retained in verification/. These failures are not relabeled as passes or whole-project closure.

Nine fresh live public HTTPS search checks passed through intrinsic investigation, child review, evidence retention and captured parent return. Model replies were controlled fixtures; this does not certify live model inference or installed Android networking. The new services/media/jobs/voice/stream checks use real local HTTP peers and actual file bytes, including a generated6MiBWAVE job result, synthetic generic-brand MP4 audio/video and image attachment bytes.

Repairs include optional service eligibility isolation, stale callback ownership, explicit file disclosure, scoped fallback and uncertain submission handling, generic host credential separation, masked credential edits, bounded streaming and disk integrity, typed job acceptance/status/result mapping, quota-managed capture originals, separate cancellable native media workers, preview context retirement, manual spoken playback, image draft attachment and detected media export filename/content type. Individual media files and job records have specific permanent-removal review; unrelated files and server jobs are preserved.

One earlier parallel run withheld a large job result during integrity verification. A subsequent retry,8 serial repetitions and8 repetitions during building passed. Its cause was not established; the strict integrity checks were retained. Verification/UPGRADE-AUDIT.json preserves that observation rather than claiming a diagnosed repair.

The signed APK retains RLLM/com.local.client.lab and the existing signing key. Both architectures, exact144build inputs, APK archive integrity, signature and16KiBalignment are inspected. Compilation and core effect checks do not establish physical microphone, decoder, touch/UI, lifecycle or live user endpoint acceptance. Those and independent current-byte review remain open. The earlier broader roadmap is preserved in the canonical engineering record and has not been declared finished.

**Python exposure and the native gate — coordinator measurement, 2026-11-12T09:4Z.** The product
no longer names the interpreter at all: `grep -rn python3 core/*.rs` returns **zero** files, where
`core/self_build.rs` carried three such call sites at the start of this window (the sanctioned unit
gate ran `python3 tools/checks.py --all-units`). `tools/gate.rs` (648 lines, 5 cases, built by
`tools/build-tools.sh`) is the native port of that one role, and the differential is byte-level, not
asserted: on one units binary `python3 tools/checks.py --all-units output/units-gate1` and
`output/tools-verify7/gate output/units-gate1` both exit 0, print the identical line
`Rust cases passed: 556/556; isolated application cases: 55`, and write `output/checks/full-units.json`
with identical bytes (sha256 `082dc397bab17162fcb03930900956f7eed0913daf5923383e733a9fc033ca16`).
Whole-tree Python is now **73 files / 7,755 lines** — `tools/` holds only `checks.py` and `package.py`,
`verification/` still holds 71 files / 7,560 lines — against **5,956 Rust insertions** in the two
commits that removed `tools/signature.py`, `tools/apk.py` and `tools/toolchain.py`. The cost is stated
with the saving: 106 lines of Python toolchain recovery became 2,211 lines of Rust, because gzip, tar
and pinned-archive verification are now implemented instead of delegated to the interpreter.
`verification/` row census, measured: 71 `.py` files = **69 group bodies + 2 shared modules**
(`live_web.py`, `transport.py`), which are imported by group bodies and are not enumerated by either
sanctioned runner — not orphan rows. Publication drift measured rather than assumed: the manifest's
`files` list pins 281 paths (233 match / 42 drift / **6 absent**, the six being the deleted Python
tools), while `build_inputs` pins 162 paths at 130 / 31 / 1 — the deletions did not add absent
`build_inputs` rows, one row moved to drift because pinned tracked sources changed content.
`apk_alignment` was re-run after the ports landed (`RLLM_CHECK_APK=… output/harness-verify apk_alignment`):
`apk_alignment PASS (exit 0, 6s, 11 checks)`, only-mode, `verification/RESULTS.json` unchanged at
`7250328b5385…`. Its group body shells only `output/tools/alignment`, so the signature/apk/toolchain
ports still have **no connected consumer** — their evidence is unit-level plus `package.sh`'s signature
call on a signed leg, which remains owner-gated.

**Two self-build defects measured in the landed journal arm (commit `55e1e96`), not absorbed.**
`append_journal_revision` writes `successor.pid = std::process::id()` — the *current* process — even when
`reload=spawn` reports a different successor pid, and the journal record carries no `job` or `instruction`
field although `core/harness.rs:358` promises that `reload=spawn` "starts a successor that inherits the job
id and instruction" and accepts `message,job,instruction` as arguments. The spawn arm passes no argv and no
environment, so gap-report step 5 ("continue the same job") is currently a pid in a receipt, not a carried
job identity. `GAP1` stays open on that evidence bar, and on reachability: `self_build` is in `const TOOLS`
(`core/harness.rs:3`) but absent from `REFINEMENT_TOOLS`, and no connected group exercises it.

**Gap stages landed in the same window (each verified by the coordinator, not the lane).** `GAP4`
stage 1 (commit `2769f19`): `reconstruct` gained `mode=behaviour`, whose document separates cited
claims from `unknown`, and `refuse_subject_copy` refuses any contiguous run of **17** subject bytes
(`QUOTED_FRAGMENT_MAX = 16` refuses before the file is created). I ran its five cases straight off the
units binary — `output/units-gate1 behaviour` selects exactly 5 tests, 5 passed / 0 failed / 551 filtered.
No connected group requests the new mode, so the eight `reconstruct`-consuming groups still witness only
the older modes; the lane's own seven-PASS only-mode run drove binaries that predate the change, so it is
a no-regression witness, not a witness of the mode. `GAP3` stage 1 moves the RRSI store off
`std::env::temp_dir()` (the durability defect I recorded when `GAP2` landed) onto the desktop state
directory (`$XDG_DATA_HOME/rllm`, matching `core/desktop.rs` `data_directory()`), and adds an
append-only `experience.jsonl` library — sealed lines with previous links, a task-keyed index, a
4,096-byte labelled prompt block that says it does not outrank the current task — written when a
refinement settles (`record_settled_library` in `finish_refinement`) and read by the next task's
propose/compose prompt (`historical_experience` beside the existing `lesson_data` injection). That
write/read pair is the consumer the gap report asks for; its verification and the `556 → 560` gate
delta are recorded on the ledger entry.

**Three gap legs land, and one connected group catches a leak the unit gate could not — 2026-11-12T11:4Z.**
`GAP1` (commit `dbddae2`): the self-build revision row no longer names the wrong process. The spawn leg appends
the revision **after** `child.spawn()`, so `successor.pid` is the child's pid, `successor.mode` says whose pid it
is (`none|exec|spawn`), and the row carries `argv`, the environment **names** handed over, and `job` / `instruction`
verbatim, with 256-byte and 16,384-byte caps refused before the edit. Witnessed through the product, not the unit
test: one model-proposed, owner-approved `reload=spawn` job in a clean room recorded revision
`450b1bef5447…` with `successor pid 2175150`, and `/proc/2175150/stat` — read while the product was alive —
reported state `Z`, parent `2171194` (the product's own pid), command `(RLLM)`. The receipt and the row use the
same value, so they cannot disagree; the independent journal reader recomputed the digest
(`digest_ok true, parent_ok true`). What remains open is stated plainly: nothing in `core` reads
`RLLM_SELF_JOB` / `RLLM_SELF_INSTRUCTION`, so the successor **carries** the job identity without resuming the job,
and the spawn arm neither waits on nor reaps its child.
`GAP4` (commit `3e59893`): `reconstruct` gained `mode=implement`, which renders a cited behaviour document into a
clean-room artifact — provenance line with the document digest and citation set, one unit line per **cited** claim,
one `refused` marker per unknown item — and a deterministic verify leg reporting `matched` / `missing` /
`conflicting` per claim. Digest mismatch, a citation-less claim, an out-of-workspace output, a fresh artifact that
does not verify, and 17+ contiguous subject bytes are all refused before the file exists. The published operation
reference (commit `20d4780`) had to be corrected because it still promised reconstruct "does not … generate an
implementation".
`GAP2` (commit `b931163`): acceptance now consults an annealed edit budget derived only from recorded ledger state
(published in the prompt, stored on the row), a noise floor equal to the largest base-to-base spread recorded for
the same task and verifier — a gain *equal* to the floor is not progress — and cost-aware acceptance that refuses an
equal score whose cost rose and keeps the cheaper one. With no budget, floor or known cost recorded, the accept bit
and notes are byte-for-byte today's behaviour.
**The methodological finding of this window, recorded because it nearly passed.** The lane's own only-mode group run
and my first re-run of eight groups both drove a product binary built **before** the change, so their eight PASSes
were a no-regression witness against stale bytes. Only after `bash headless.sh` rebuilt the product did
`hosted_phases` go red: `AssertionError: evaluation does not retain the task, answer or verifier in the candidate`
(`verification/hosted_phases.py:85`), with `COUNT UNPARSED` because the group died mid-run. The cause was in the new
code: `record_base` stored the **raw task string** in `run.base_evaluations`, and that run state is held in the
refinement candidate — a leakage invariant, not a style rule. Repaired in `2b80ee8` by keying those rows with
`task_key` (a digest) plus the existing verifier digest, keeping the floor's spread semantics. Re-measured on
rebuilt bytes: `hosted_phases PASS (exit 0, 1s, 118 checks)` and eight groups 8 pass / 0 fail. **Rule taken from
this:** an only-mode group run against a binary built before the change is a no-regression witness only; a change
to product state shape must be re-run after `bash headless.sh`.

**Four gap legs land, and the recursion loop catches a boundary defect in its own new code — 2026-11-12T13:3Z.**
`GAP1` (commit `3390359`): the successor now **consumes** the job it was handed. `inherited()` is the only
reader of `RLLM_SELF_JOB` / `RLLM_SELF_INSTRUCTION`; a caller-supplied `job` still wins, an omitted one takes
the carried value for the journal row **and** for the next handover, over-cap carried values refuse before the
edit, and `reload=exec` no longer drops the identity it had just validated (the arm used to call
`execution::exec_replace(&binary, &[], root, &[])` — an empty environment). Witnessed end to end in a room
archived from that commit (`docs/SELF-BUILD-RUNLOG.md`): a `reload=spawn` job recorded revision
`abff31fd087c…` naming successor pid 3061720; `/proc/3061720/stat` showed state `Z`, ppid = the product, comm
`(RLLM)`; the next self-build in the same process appended revision `630c16acf19e…` with parent
`abff31fd087c…` and the receipt clause `reaped 1 successor(s)`, and `/proc/3061720` was then **gone** — a
zombie before, reaped after, read from outside the product. Four journal rows, each digest recomputed by an
independent reader, each `parent` equal to the previous row, with a no-environment control recording
`job_source none` so the new key cannot be a constant.
**The same run went red on the next leg, and that is the finding:** a successor process carrying exactly what
the spawn arm hands over refused its own gate — `reload refused: unit gate is not green (579/581)`. Same room,
same unit binary, `--test-threads=1`, only the two variables changed: 16 failures without them (the bare-run
fixture failures) versus 18 with them, the two extra being
`self_build::checks::journal_mode_is_none_without_reload_and_exec_records_exec_only_when_it_writes` and
`self_build::checks::spawn_journal_stores_job_and_instruction_verbatim_and_absent_job_is_empty`. The seam's
builder and gate children inherit the parent environment, so an identity meant for a successor product reached
the product's own test children. A second boundary defect came from the same room: an edit that does not parse
emits 7,078,364 bytes of rustc stderr and the seam refuses with `builder output exceeded its bound` **before**
it reports the child's exit status, so a broken edit hides its own compile error. Both are open on the `GAP1`
ledger entry.
`GAP2` (commit `600e44d`): three more RRSI features — every edit records its component, hypothesis, diff digest,
both scores, both costs and the verdict, and a rejected component+hypothesis pair cannot be proposed again in
the same run; a leakage critic runs **before** any score is computed, refusing a candidate that reproduces a
recorded task, quotes 17 or more contiguous bytes of an expected value, or changes nothing in the component it
claims; a component with no strictly positive gain across a three-round window is pruned and refused
afterwards. 16 new cases; the whole tree gates at **597/597 + 55**.
`GAP4` (commit `d8d0881`): `pe_declarations` now drives `reconstruct mode=behaviour` and `mode=implement`
through the product — provenance digest, unit/refused markers counted from the artifact bytes, independently
recomputed `matched`/`missing`/`conflicting` verdicts, and every refusal leg stats the artifact afterwards to
prove it was never written — taking that group from 18 to **28** recorded checks. Two refusal legs are caught at
the reviewed-dispatch permit rather than at the render seam, so they prove refusal and absence, not the render
message; that limitation is recorded rather than papered over. **The delivered record has not moved yet:** the
18 → 28 delta needs a coordinator sweep, and the record's `total_executed` will follow it.
`GAP5` (commit `dc62928`): the publisher is now a native tool. `tools/package.rs` (2,630 lines, 17 cases run by
`tools/build-tools.sh`, which now builds **8** tools twice and requires executed == declared for each) derives
the group inventory from `tools/checks.py` instead of trusting a count, enforces the executed-count arms, names
the path it refused on, and adds `--dry-run`, which prints the pinned-input censuses and writes nothing.
Measured by me on this host: `build_inputs: pinned 162 match 129 drift 32 absent 1` and
`files: pinned 281 match 231 drift 44 absent 6`, then `refusing: output/checks/apk.json absent`, exit 1, and
`/tmp/pkg-dryv` left empty. `tools/package.py` stays — deleting it is gated on a real native publish, which
needs the owner's signed companion APK. Python exposure is unchanged this window: **73 tracked `.py` files /
7,755 lines**, 71 of them under `verification/` (69 connected groups plus `live_web.py` and `transport.py`),
zero `python3` references under `core/`.
Method held: each lane was verified on attributable bytes before commit — lane C3 in a room holding only its own
file (581/581 + 55), lane B3 on the whole tree (597/597 + 55, built and gated by me, not by the lane) — and
after all four landed the product was rebuilt and `commands`, `hosted_phases`, `provider_evidence` and
`pe_declarations` were re-run on the new bytes: **4 pass, 0 fail** (79 / 118 / 40 / 28 checks).

### GAP1 closes its last proof leg; the pool lands and immediately shows a live-reachability hole — 2026-11-12T14:5Z

`GAP1` (commit `63bf5c5`): the boundary defect the chain witness caught is fixed and the **same witness,
re-run on the rebuilt room, is now green end to end**. A parent product spawns a successor (receipt names
pid 3141890; `/proc/3141890/stat` reads state `Z`, ppid equal to the product, comm `(RLLM)`), the next call in
the same process prints `reaped 1 successor(s)` and the `/proc` entry is then absent. A fresh product process
carrying only what the spawn arm hands over — and giving no `job` argument — now passes its own gate
(**585/585 + 55**, where the same leg before the fix printed `unit gate is not green (579/581)`) and journals
`job gap1-chain-carried` with `job_source inherited`; the no-environment control still journals
`job_source none`, so the key discriminates. All eight journal rows re-digest under an independent reader and
each `parent` equals the previous `revision`. Lane C4 also stopped the unit cases mutating the process-global
environment, which the coordinator measured as **25 multi-thread failures at `f2a6ad8` → 19 at `63bf5c5`**,
five of them the named carried-environment cases and **zero** new names. The whole tree at `63bf5c5` gates
**611/611 + 55**.
`GAP2`/`GAP3` (commit `f2a6ad8`): a round can now answer with a bounded pool of up to three candidates, each
criticised before it is scored, and each candidate is committed in its own `git worktree` under
`.rllm-worktrees/` — the accepted one fast-forwards the `rllm-evolve` branch while the caller's `HEAD`, index
and work tree stay untouched, verified by a case that compares `rev-parse HEAD` and `status --porcelain`
before and after. Held-out and out-of-distribution legs are scored separately and a missing leg is recorded as
unknown rather than as a pass; `resume_run` rebuilds the prompt, adopted harness, prune and rejection state and
the annealed budget from the ledger and the sealed library alone, and a tampered library entry refuses with the
existing bytes proven unchanged. Verified alone in a room holding only those three files: **607/607 + 55**,
which is 597 plus the 10 new cases, and the re-run's own ledger rows show the three pool outcomes — incumbent
refused for changing nothing, one accepted row with `branch rllm-evolve` and a 40-hex commit, and a leaking
candidate refused for reproducing a recorded task.
The defect that came out of reading that code, measured rather than inferred: `evaluate_pool` reads
`run["workspace"]` (`core/harness.rs:724`), but that key is written **only inside `mod checks`** of the same
file (`:1958`, `:1975`, `:1997`); the live `refine` entry that builds `task_run` (`core/harness.rs:400`) never
sets it. So every real refinement round takes the empty-workspace arm — one recorded refusal per candidate,
`incumbent_kept true`, and no `commit_candidate`, no `fast_forward_evolve`, no `rllm-evolve` branch. The pool's
scoring and critic legs do run live; its git legs are unreachable. Lane F is closing that by putting the
product's own workspace authority (`approval_root`, the same key `core/claim_records.rs:83` records as
`workspace`) on the refine `task_run`.
Related census for the same surface: **no connected verification group exercises the improvement loop at all**
— `grep -rln "experience.jsonl|edits.jsonl|rrsi" verification/*.py` returns nothing — even though the loop is
product-reachable (`harness::request_messages` at `core/application.rs:852`, `harness::model_reply` at
`core/application.rs:2657` and `core/tool_format.rs:186`). That is the same reachability hole the new
`self_rebuild` group is closing for `self_build`.
Python exposure this window: **73 tracked `.py` files / 7,807 lines**, +52 lines since the 13:3Z measurement,
all of it `verification/pe_declarations.py` (+55/−3 in `d8d0881`) — the R-rule tension is recorded, not hidden:
the connected groups are still Python, `tools/package.py` is still the publisher of record until a real native
publish replaces it, and `core/` still has zero `python3` references.

### The full check on committed bytes, what the sweep does to the record, and the audits read this window — 2026-11-12T15:4Z

**The full check ran and came back red at the tools stage.** On pinned `HEAD` bytes in a `git archive` room
(nothing uncommitted): `bash tools/build-units.sh` + `python3 tools/checks.py --all-units` → **611/611 cases +
55 application cases, exit 0**; then `bash tools/build-tools.sh` → **exit 1** because the gate tool's own test
`failed_batch_records_the_output_tail` panicked at `tools/gate.rs:620` with
`"cannot start /tmp/rllm-gate-3188754-2/units: Text file busy (os error 26)"` — it execs a `#!/bin/sh` fixture
it has just written. Because that stage failed, `bash tools/build-harness.sh` and the group sweep never ran.
Isolation trial: three consecutive `bash tools/build-tools.sh` runs in one room, all eight tools, twice each →
`Text file busy` count **0**, failures **0**, exit 0. So this is a load-dependent race (about one red in four
runs under fleet load), not a flaky test to be waved through; the bounded fix belongs in the gate tool.

**What a sweep does to the delivered record, measured rather than assumed.** Two rules in `tools/harness.rs`
were confirmed by reading and by an actual run:
- `tools/harness.rs:1047` publishes only when `only.is_empty() && problems.is_empty() && failed == 0 &&
  unparsed.is_empty()`. **One failing group blocks republishing the record entirely** — the record keeps the
  old census instead of half-updating. That is why the 70-group census cannot land while `self_rebuild` leg 4
  is red.
- A sweep whose groups are all *unavailable* still publishes, and it **hollows the counts it writes**. Running
  `./output/harness-run` in a room with no `output/headless/RLLM` printed `summary: 0 pass, 0 fail, 69
  unavailable of 69 groups` and then `delivered record written: 69 groups, 0 checks`. Diffing that room's copy
  against the delivered file: **69 leaf paths changed**, every `connected.counts.<group>` from a real number
  (`commands` 79, `hosted_phases` 118, `pe_declarations` 18, `permission_modes` 27 …) to `0`, with the
  runner's own `zero_count_disclosure` naming the unexecuted groups. The delivered record at this root stayed
  byte-identical (`7250328b…` before and after). Delivered state read in full: `connected` 69 groups,
  `total_executed` **4,185**, `groups_passed` 65, four groups recorded 0 (`binary_flow`, `history_migration`,
  `apk_alignment`, `turn_background`). The working tree declares **70** names; the extra one is the
  uncommitted `self_rebuild`, and it is the only name in the runner that is missing from the record.
- Consequence for the standing rule: after a sweep the witness is the `delivered record written:` line plus the
  file digest, **not** the `recorded in verification/RESULTS.json:` line, which is a pre-run read.

**The independent audits read this window changed three things.**
- **A claim of mine was refuted and is corrected above**: `full_rust_suite` is written by both packagers
  (`tools/package.py:52`, `tools/package.rs:1908`), not only by `package.py`.
- **The `self_rebuild` group passed its interlock audit and failed for a reason the group itself is right
  about**: 8 checks, 7 pass, 1 fail, reproduced. The red leg is a product ordering defect —
  `core/self_build.rs:1343` `run()` calls `1398` (apply the reviewed edit) **before** `validate_with`
  (`1008`–`1026`) refuses an unsanctioned builder (`sanctioned_child:350`), so a refused self-build still
  mutates the source. The audit's non-blocking caveats on the passing legs are recorded: it could not rebuild
  the product, so legs 6/7 (spawn/reap) were judged against a binary that predates `63bf5c5`; its claim that
  `checks.py` reports 37 of 70 groups was misread by me as a coverage gap — that array is the `--group` CLI
  list and the coverage census is the `names` list asserted against the runner at `tools/checks.py:2265`.
  The audit also flagged undeclared co-resident changes (`core/harness.rs`, lane F's workspace fix) and
  untracked artefacts in the tree — both real, and both why `self_rebuild` will be committed as
  `verification/self_rebuild.py` + the four interlock files only.
- **The MiniMax GAP3 findings were adjudicated, not swallowed.** Confirmed: `commit_candidate`/
  `fast_forward_evolve` failures propagate with `?`, which discards the ledger rows that `note_refinement_edit`
  (`core/harness.rs:102`) and `critic_against` (`core/harness.rs:147`) had already attached — one rejected
  candidate can erase the round's ledger; and `commit_candidate` (`core/rrsi.rs:1604`) only commits into a
  scratch worktree, so a pool winner is *recorded*, never *applied* to the source. Refuted: the claim that the
  three-slot cap makes round 2 abort — `commit_candidate` calls `remove_slot` on the same slot first, so
  `worktrees` stays ≤ 3 across rounds. The related point stands: an aborted round leaves its slot directories
  behind, and `require_worktree_root` (`core/rrsi.rs:1505`) treats a stale directory as usable.

### Four verified lanes land; the tools stage is now green on a clean tree — 2026-11-12T15:5Z

Every lane below was rebuilt **alone** in a `git archive` room holding only that lane's file, so each case
delta is provably the lane's own, and nothing was committed before an audit was read in full.

| Lane | File | Verified alone | Commit |
|---|---|---|---|
| H | `core/self_build.rs` | **616/616 + 55**, `self_rebuild` group **8/8** | `a9c62c5` |
| E6 | `verification/self_rebuild.py` + 4 interlock files | census **69 → 70**, group runs in only-mode and full mode | `2c6b792` |
| F+F2 | `core/harness.rs` | **620/620 + 55** | `5b898cd` |
| I | `tools/gate.rs` | 3 consecutive tools builds exit 0, `Text file busy` count **0** | `3e5eda1` |
| J | `tools/toolchain.rs` | clean room with **no `tooling/` at all**: all 8 tools, exit 0 | `112e251` |

**The clean-tree tools defect was the real blocker, and it was not the race.** `tooling/` is gitignored
(`.gitignore:14`), yet `tools/toolchain.rs:2133` read `tooling/env.sh` and unwrapped it, so on a tree with
nothing untracked the toolchain tool's test panicked with `NotFound` and `tools/build-tools.sh` stopped before
the tools listed after it — including `output/tools/gate`. That single panic is why the unit-gate tool, the
harness runner and every connected group that needs the gate cannot be built from a clean checkout. Reproduced
twice independently: in the room where `tooling/` was symlinked in, the toolchain leg passed (6 cases) and only
the gate race remained; in the clean room, the fixed file built all eight tools and the toolchain leg reported
7 cases.

**The ETXTBSY race is more common than the first measurement suggested.** The follow-up run hit it in *two*
tests of the same tool in one pass (`tools/gate.rs:576` and `:620`, `green_run_writes_report_and_gate_line` and
`failed_batch_records_the_output_tail`), not one, and it reproduced again in the second tools build of the clean
room while that file was still unfixed. The fix closes the fixture handle before setting its mode and retries
only `ErrorKind::ExecutableFileBusy` / raw os error 26, at most five times, 5 ms apart.

**Audits read this window (all read in full, none swallowed).** MiniMax seats over H, F, I and J returned
PASS-WITH-CAVEATS with no BLOCKER or MAJOR, and no new external dependency in any of them — the only outside
program anywhere near these paths remains the bounded `git` run reached from `core/rrsi.rs`, already recorded as
an open R1 decision. Two MINOR coverage gaps are now open items rather than accepted silence:
the gate retry is tested through the helper, not through `run_cmd`'s real spawn path; and the `tooling/env.sh`
pin leg now runs only when that file exists, so a clean tree pins the *writer* instead of the checked-out file —
acceptable because the sanctioned build environment is an installed tree, but stated rather than assumed.
The F audit's two missing legs are queued as GAP3 work: `approval_root` provenance (a *valid but different* git
worktree as the workspace, plus symlink/relative-path variants) and a production-dispatch witness that the live
refine action reaches the entry that now puts `workspace` on the task run — the latter is exactly what the
connected RRSI-loop group has to supply.

**The full check on `HEAD` bytes is green again.** Re-run in a `git archive` room of `4d60911`:
units **exit 0**, unit gate **620/620 + 55 exit 0**, `bash tools/build-tools.sh` **three consecutive runs exit
0 with a `Text file busy` count of 0**, harness runner **exit 0**. The sweep leg in that room reported
`0 pass, 0 fail, 70 unavailable of 70 groups` because the room had no product binary — and it again published a
hollow record (`delivered record written: 70 groups, 0 checks`), so the room copy's counts went to zero while
the delivered record stayed at `7250328b…`. The runner printed
`INVENTORY DRIFT: the delivered report counts 69 groups while the runner has 70`, which is the honest way to
learn that the record has not been republished yet. A sweep with real group counts needs the product binary in
the same tree, so it runs once in the main tree after the current lanes land.

### Gap 4's cross-reference layer lands, and an audit's REFUTED verdict is itself refuted — 2026-11-12T16:1Z

`a2a976e` adds call edges and cross-references to native binary analysis, implemented in Rust inside the product
(no `objdump`/`readelf`/`nm`/`addr2line` anywhere — measured, not assumed). A direct edge is kept only when two
independent resolution routes agree on the same address; disagreement and overflow become unknowns that state
their reason, and `BR`/`BLR`/`RET` plus register-authenticated forms are unknowns rather than guessed edges.
Verified alone in a room of `HEAD` plus its three files: **629/629 cases + 55**, which matches the `#[test]`
marker census in tracked `core/` (629) exactly.

The MiniMax seat on that diff returned **REFUTED**, and its reasoning was mostly about evidence scope rather than
about the code, so each of its objections was checked against the tree instead of accepted:
- "the PE AArch64 production path is not shown" — **refuted by measurement**: `core/pe_observation.rs:126` calls
  `super::control_flow::analyze(...)`, and `core/pe_observation.rs:105` passes a
  `super::control_flow::Observation` through the PE observe path. The diff simply did not contain those files.
- "the exact production chain through `investigation.rs` and `elf_observation::observe_with_cancel` is not shown" —
  **refuted by measurement**: `core/investigation.rs:102–103` reaches `elf_observation::observe_with_cancel` /
  `analyze_with_cancel`, and `core/elf_observation.rs:166` calls `control_flow::observe` and appends
  `context.report` onto the observation text, exactly as claimed.
- "the unresolved-disagreement route is not exercised by a test" — **correct and kept**: the existing
  `unresolved_direct_target_is_unknown_not_an_edge` (`core/control_flow.rs:341`) drives the *overflow* arm
  (`reason.contains("overflow")`), so the `(Some(_),Some(_))` disagree arm at `core/control_flow.rs:75` had no
  coverage, meaning the two-rules-must-agree invariant could regress silently. A follow-up lane (`bg_8d39f05e`)
  is closing it and is additionally required to prove that every output cap discloses its own truncation, so a
  partial call graph cannot be read as a complete one.

### A sweep at `HEAD` with the product binary: 63 pass, 0 fail, and my own publish-rule sentence corrected — 2026-11-12T16:2Z

To de-risk the record republish before touching the delivered file, the whole chain ran in a `git archive` room of
`1622ab4` **with** the product built inside it (`bash headless.sh` exit 0):

| Stage | Result |
|---|---|
| `bash tools/build-units.sh` | exit 0 |
| `python3 tools/checks.py --all-units` | **629/629 + 55**, exit 0 |
| `bash tools/build-tools.sh` | exit 0 |
| `bash tools/build-harness.sh` | exit 0 |
| `./output/harness-run` (full sweep) | exit 0 — `summary: 63 pass, 0 fail, 7 unavailable of 70 groups` |
| room record after publish | `delivered record written: 70 groups, 4,108 checks` |

Two things follow. First, **nothing fails at `HEAD`**, so a main-tree sweep will republish the record cleanly; the
70-group census and the corrected `pe_declarations` count go in together. Second, the sentence I wrote earlier
today — "the record will not be republished until a full sweep runs with every group executable" — was **wrong**,
and it is corrected above: `UNAVAILABLE` groups never block publication. They are written as `0` with
`missing` inputs listed and a `zero_count_disclosure` line naming them, which is what the delivered record's four
zero entries are. The blockers at `tools/harness.rs:1047` are a failed group, a census `problem`, or an unharvestable
count. The practical hazard is the mirror image of what I wrote: because publication does happen with unavailable
groups, a sweep run somewhere that lacks reference inputs silently rewrites real counts to `0` — the seven groups
that went to zero in this room (`binary_flow`, `history_migration`, `field_flow`, `flags_flow`, `product_flow`,
`apk_alignment`, plus the off-host signed-APK leg) are the same inputs already tracked as owner-gated
(`output/previous-controller`, `output/pre-branch-controller`, `output/pre-auth-controller`,
`output/published-controller`, and the companion APK named by `PUBLICATION-MANIFEST.json`). A room can therefore
never produce the delivered record's real totals; only the main tree, which holds those reference builds, can.

### GAP1's last leg closes: a successor's spawn and exit survive the process that recorded them — 2026-11-12T16:4Z

`12cdc6b` makes the handed-off successor observable. Spawn and reap rows go into a **sibling** content-addressed
journal, `self/journal.successors.jsonl`, derived from the caller's revision-journal path — deliberately not into
the revision journal itself, because that file's last line is the parent the next build must match and the existing
witnesses count one row per successful build, so inserting rows there would move the parent chain. Rows reuse the
file's own `append_record` / `journal_lines` / `tail_revision` rules (`schema "1"`, `parent` = the previous row's
`revision`, `revision` = sha256 of the object with `revision` removed). Spawn rows carry `pid`, `job`,
`source_digest`, `mode`, `timestamp`; reap rows carry `pid`, `job`, `exit`, `signal`. The spawn row is written only
after `child.spawn()` succeeds, and `durable_successor_rows` reads the sibling **alone** — verifying every digest
and parent link, touching nothing in the process table, and returning an empty list without creating a file when the
journal is absent. A refused self-build leaves both journals absent. Verified alone in a room of `HEAD` plus that one
file: **633/633 cases + 55 isolated application cases**, exit 0 (629 + its 4 new cases).

The MiniMax seat on this diff was **cut short by its own token limit** (13,094 bytes, stopping mid-finding F3, so its
masking / dependency / test-adequacy sections never ran). That is recorded rather than glossed, and I ran its missing
R1 leg myself: grepping the diff's added lines for `Command::new` / `bounded_process` / `process::Command` returns
nothing, so the change introduces no outside-program dependency. Its three findings were adjudicated, not accepted:
- **F1 (called High)** — a spawn row can outlive a failed revision append. Kept as a design consequence rather than
  a defect: the child really was spawned, so the row is not a false claim. The rule is that the sibling journal is
  evidence of a spawn/exit, never of a completed handoff, and it joins to the revision journal through `job` and
  `source_digest`.
- **F2 (Medium-High)** — the reap row is written before the process-local table is updated, so a crash mid-reap
  leaves the durable record ahead of the dying process's table. That is the intent of making it durable; recorded.
- **F3 (Medium) — real, and dispatched.** `reap()` (`core/self_build.rs:1426`) collects ready children, then appends
  with `?` (`:1452`) *before* clearing `tracked.child`. A failed append therefore returns before the state
  transition, and because `Child::try_wait()` returns the cached status afterwards, the next `reap()` journals the
  same exit a second time. Repair lane `bg_ed8ed3e7` reorders it (state first, row second, failure surfaced once) and
  must also add the tamper legs the audit noted were untested: a flipped byte inside a reap row, a wrong `parent`, and
  a row removed from a three-row chain.

### Cross-reference caps named, and the new loop group convicts three things — 2026-11-12T16:5Z–17:0Z

`c34b170` closes the GAP4 truncation-disclosure leg in `core/control_flow.rs`: `xref_text` used to print only
`set.refusal`, so when the edge cap fired first the reader never learned that the unknown list, the per-symbol
edge lists or the 4 KiB report had also been cut. It now recomputes both — per-symbol `unknown` tallies against
the stored unknown list, and each symbol's incoming/outgoing edges against its stored lists — and appends the
cap's own sentence. The refusal strings themselves are byte-unchanged. Verified alone in a room of `HEAD` plus
that one file: **638/638 cases + 55 isolated application cases**, exit 0 (633 + its 5 new cases), with the marker
census moving 8 → 13 in that file as the second, independent attribution instrument.

The MiniMax seat on this diff is text-only (it sees the diff, not the repo), so its verdicts were adjudicated
against measurement rather than accepted:
- **Claim 2 REFUTED, and my own wording was the error.** I had claimed the recount runs against *the untruncated
  edge set*; it runs against `set.edges`, which is itself capped by `XREF_MAX`. For the per-symbol list cap that
  is the right semantics (the list cap is per-symbol, `XREF_SYMBOL_LIST = 8`, independent of the 64-edge cap), but
  it does not detect a cut in the edge source — that is what `set.refusal` names. Corrected above and in the
  follow-up brief, which now requires a case where the edge cap did **not** fire.
- **Its vacuity objection is real for two cases, and I settled it with a mutation it could not run.** Deleting
  both new disclosure blocks in a room copy flips exactly one case —
  `application::control_flow::tests::later_caps_are_named_beside_the_first_refusal` (638 → 637/638, gate exit 1).
  So the new code is load-bearing, but `unknown_cap_names_the_refusal_in_the_report` and
  `symbol_list_cap_names_the_refusal_in_the_report` pass without it: in their fixtures the cap under test *is* the
  first refusal, so the pre-existing `set.refusal` path prints the sentence anyway. Follow-up lane `xrefo` must
  make each discriminate on its own.
- **Claim 4 measured by me, not by the seat:** the `disagree` arm (`core/control_flow.rs:75`, displaced and
  coordinate arithmetic both yield a target but they differ) is still **uncovered** — every occurrence of
  `"disagree"` in the tests asserts its *absence*. Also dispatched in `xrefo`.

The new `rrsi_loop` group (uncommitted: `verification/rrsi_loop.py` plus the 71-group census wiring in
`tools/checks.py`, `tools/harness.rs`, `tools/package.py`, `tools/package.rs`) is sound as a registration — the
room's unit gate stayed green at 638/638, which is what `checks.py`'s assertion against the runner's own list
certifies — and it convicted three things on its first live run:
1. **My instrument was stale, not the product.** The group found `task_run.workspace` absent on `refine` while the
   on-disk `core/harness.rs:400` sets it. The reason is that `output/headless/RLLM` is dated **06:13**, i.e. it
   predates `5b898cd` (07:31) and every product commit since. So the sweep witness I recorded at 08:1x
   (63 pass / 0 fail, 4,108 checks) measured a product binary that predates four landed commits. Nothing about
   the census or record mechanics changes, but the connected legs of that sweep are an older product's behaviour,
   and the republish run must come **after** `bash headless.sh` rebuilds the binary.
2. **A wrong expectation in the group, not a defect.** The live loop writes its edit ledger as
   `{run_digest}.jsonl` under the default root; `edits.jsonl` is never created (`commit_edit` picks the digest
   filename when the ledger root is empty). `experience.jsonl` does gain a sealed row for the settled run.
3. **A real GAP3 defect the group found.** `rrsi::resume_run` is **not on the JSON-lines path** — its only callers
   are two unit tests in `core/rrsi.rs`. A fresh process with the ledger and `experience.jsonl` on disk reads the
   library into the propose prompt but never rebuilds the ledger prompt, so a refinement cannot be resumed across
   processes. That is the "resume across tasks" leg of GAP3, now with an instrument attached to it.

### Two GAP3 defects closed, and the loop group narrows to one real red — 2026-11-12T17:1Z–17:2Z

`4758d28` (`core/self_build.rs`) closes the F3 ordering defect the previous audit found: `reap()` used to
append the durable reap row with `?` **before** clearing `tracked.child`, so a failed append returned before
the state transition and the next `reap()` journalled the same child exit twice. The transition is now
committed first and a failed append costs only a missing witness row. It also closes the three tamper legs the
audit listed as untested, each designed so the intended check is the one that fires — the flipped reap payload
stays parseable (content-address check), the rewritten parent has its digest recomputed (only the link check
can fire), and the dropped middle row leaves both survivors individually content-addressed.

`5c93aed` (`core/rrsi.rs`, `core/harness.rs`) closes GAP3 defects 1 and 2: one candidate's git failure used to
abort `evaluate_pool` and lose every ledger row for the round — the refusal is now carried in that candidate's
own pool row (`commit_candidate:` / `fast_forward_evolve:` / `apply_accepted:` / `incumbent_base:`, truncated at
240 chars) with `commit`/`branch` left empty, so a row cannot claim a commit that did not happen; and the
winning candidate is now actually applied to the caller's file after the fast-forward, guarded by the source
digest recorded at candidate time and verified by read-back (`candidate target write did not land`). Slot
directories are discarded when a round does not finish, a stale slot is re-initialised rather than reused, and a
path containing `.rllm-worktrees` is refused as a workspace.

Attribution, measured with one lane's file per room: `4758d28` alone → **642/642 + 55** (= 638 + 4);
`5c93aed` alone → **645/645 + 55** (= 638 + 7). Both rooms then rebuilt the product and the harness runner,
because `output/headless/RLLM` is still the 06:13 artifact.

Two instrument corrections from those runs:
- A group that needs `output/tools/gate` reports **UNAVAILABLE** in a room that has no tools build, so the
  first `self_rebuild` witness was vacuous. After `bash tools/build-tools.sh output/tools` inside the room, the
  same run gives `self_rebuild PASS (exit 0, 1s, 8 checks)`. Rooms that run git- or gate-dependent groups must
  build tools, product and runner, not just units.
- With a rebuilt product plus `5c93aed`, the `rrsi_loop` group narrows from three reds to **two of six**:
  `refine entry copies approval_root onto task_run.workspace` now **passes**, which confirms the stale-binary
  diagnosis rather than a product defect. The remaining reds are the group's `edits.jsonl` naming expectation
  (production writes `<run_digest>.jsonl`) and the cross-process resume leg, which is the real defect.

The resume defect is what the group's last red convicts: `rrsi::resume_run` has exactly two callers, both unit
tests, so a fresh process reads `experience.jsonl` into the propose prompt (`harness_in_prompt=True`) while
rebuilding nothing from the ledger (`edit_history=False`, `phase=probe`) and re-proposes a hypothesis the
previous run already rejected. Dispatched as lane `resume` (`core/rrsi.rs`, `core/harness.rs` only).

Audit adjudication for these two landings: the `succfix` seat CONFIRMED claims 1–2 with quoted lines and found
no defect, so its NOT-MEASURED claims (repo-wide string stability, census) were measured by me instead —
refusal strings byte-unchanged, room census 642/642. The `pooll2` seat could not confirm most claims from a
34 KB diff, so I measured its open points: the diff's two added `std::process::Command::new("git")` lines are
test-only scratch-repo helpers while production git stays on `git_run` → `execution::BoundedOutput`; its
"pool row claiming a commit that did not happen" is refuted by reading `refuse_pool` and `push_pool_row`; and
`apply_accepted`'s guard order is size/NUL → slot path → digest compare → write → read-back. One limitation is
recorded rather than papered over: the digest compare and the write are not atomic (no rename, no `O_EXCL`), so
a writer landing between them is not caught — acceptable for this single-process route, but not atomicity.

## The unit line is re-pinned at 649, one lane's cap tests land, and the resume gap is the only red — 2026-11-12T17:3Z–18:0Z

The carried case count was stale. Measured in a `git archive HEAD` room at `e4b7452`, the sanctioned line is
**649/649 cases plus 55 isolated application cases**, not the 645 I had been carrying since `5c93aed`. The room
number is HEAD's own: `tools/build-units.sh` resolves `project_dir` from `$0/..`, and `tooling/env.sh` exports
toolchain paths only (`CLIENT_RUST` etc., derived from its own directory) — neither pins the project root, so a
room cannot silently compile the main tree. The tracked-`core/` marker census is 646 while the case count is
649, which is the same lesson as before: markers are a per-file *delta* instrument, never a case total. The
+4 over 645 came from commits after `5c93aed` and was **not** re-derived per commit.

`xrefo` landed as `98fb0d9` (`core/control_flow.rs` only): room 649 → **650/650 + 55**, and the
`control_flow::tests` filter selects 14 cases where the file holds 13 markers at HEAD. Its leg A is a negative
result, reported as such instead of being papered over: the `(Some(_),Some(_))` disagree arm at
`core/control_flow.rs:75` is unreachable. I proved that algebraically rather than trusting the lane's sweep —
`Value::coordinate` builds `{bits, known: u64::MAX, relative: true}` (`core/instruction.rs:70`) and `full()`
only re-tests `known == u64::MAX` (`:75`), while `coordinate_delta` is the same `checked_sub`/`checked_add`
as `displaced` (`:146`, `core/control_flow.rs:35`), so `coordinate_target` is the identity on `displaced` and
the two arms can never disagree. The lane's sweep (744,154,436 agree / 338,238,612 overflow / **0 disagree**
across every B/BL imm26, B.cond/CBZ imm19 and TBZ imm14 at 16 program counters) corroborates it but is only
sample evidence; the algebra is the load-bearing witness.

The 71-group census is de-risked on the main tree: `tools` → runner → `self_rebuild PASS (exit 0, 1s,
8 checks)` with `INVENTORY DRIFT: the delivered report counts 69 groups while the runner has 71`, i.e. the
packager inventories accept the 71st group while the delivered record still describes 69.

`k2`'s rewritten `rrsi_loop` group was verified the same way every other lane was: a fresh `git archive HEAD`
room with only `verification/rrsi_loop.py` plus the four census files copied in. Units stayed at **649/649 + 55**
(correct — the lane changes no `core/` file), and after building tools, product and runner *inside* the room:
`self_rebuild PASS (exit 0, 1s, 8 checks)`, `rrsi_loop FAIL (exit 1, 0s, 6 checks)`, `summary: 1 pass, 1 fail,
0 unavailable of 71 groups`. Exactly one leg is red — `files alone rebuild the resumed phase and decision` —
and the gap is real, re-measured at HEAD: `rrsi::resume_run` exists at `core/rrsi.rs:1374` with its only two
callers at `:2516` and `:2556`, both unit tests.

Audit adjudication for `k2`: the seat read the whole 17.7 KB group and CONFIRMED all six legs as substantive,
quoting the independent digest recomputation (`body={k:row[k] for k in row if k!='revision'}` then
`sha256(canonical(body))`) and the `previous`-chain assertion, and it specifically rejected the worry that the
red leg could pass on bad wiring — it requires the recorded hypothesis in the prompt *and* a non-`probe` refusal
naming the component and round. Two limitations are recorded rather than hidden: the refine-entry leg exercises
only the *absent*-source digest case (`rllm-harness.txt` missing), so a non-empty source digest is untested; and
the resume leg pins exact prompt markers and exact refusal wording, which is spec brittleness, not vacuity. The
legs it could not measure from a diff (the live ledger naming, the single-red run, the unit gate) are the ones I
measured above.

Commit interlock, deliberately: the group and the 71-group census stay uncommitted until the `resume` product
fix lands, because `tools/harness.rs:1047` publishes only when `problems` and `failed` are empty — a FAILING
group would block the record, so committing them alone trades one drift line for a blocked publish.

Lanes in flight: `harq` (new `core/har_evidence.rs` + `core/lib.rs` mod + a consumer in
`core/observation_records.rs`, GAP4 stage 5), `resume` (`core/rrsi.rs`), `pkgp5` (`tools/package.rs` parity
census), and `js3` (GAP4 stage 3, JS/Electron observation — measured absent before dispatch: `grep -rli
"electron|asar|source map|sourcemap" core/` returns nothing, and the only `javascript` hits in `core/` are web
MIME handling). GAP4 stages 1 and 2 and the `pe_declarations` connected witness are already landed, so stage 3
is the next real slice, dispatched with a consumer wired in the same change.

## The JavaScript lane lands, a .NET consumer is rejected as dead wiring, and the record still has mixed provenance — 2026-11-12T18:3Z–20:0Z

`rb5` landed as `478e9f5`: `tools/readback.rs` went from 1 to 24 `#[test]` markers (inflate over
stored/fixed-Huffman/dynamic-Huffman streams, truncation and over-size refusals, real-ZIP fixtures, a
corrupt-byte witness that names the reserved block type, little-endian field readers, `py_repr` and
`PyErr` parity). The claim "no runtime change" is confirmed by hunk arithmetic: the only hunk is
`@@ -1401,0 +1402,359 @@` inside `#[cfg(test)] mod checks`, which starts at :1389.

`js3` landed as `db16ffa` (`core/js_evidence.rs` 52,344 B with 5 markers, the `core/lib.rs` mod line, and
`core/investigation.rs`). Verified in a `git archive` room of the commit itself: `Rust cases passed:
672/672; isolated application cases: 55`, exit 0, which is HEAD's 667 + 5, equal to the new file's marker
count. Its audit seat answered questions A–D and then hit its token ceiling, so legs E and F were measured
here instead of accepted: `core/investigation.rs` is exactly 2 hunks (the shared `NATIVE_CAPABILITIES`
advertisement and the 4-line JS branch whose `cancelled()` guard precedes the work), and `js_evidence.rs`
contains no `Command::new`, `std::process`, `std::env`, `std::net`, filesystem access, `unsafe` or `extern`
block — its only import is `super::{parse, Json, Json::*}`. Two limitations are recorded rather than
hidden: package-field and IPC happy paths ride the inspect route with no unit of their own beyond the
inert capability census case, and the shared capability string did change, so no whole-tool golden byte
diff was taken. `core/lib.rs` was staged with only the `js_evidence` mod line because a sibling lane's
uncommitted `mod dotnet_evidence;` shares that file; committing it whole would have referenced an
untracked source file and made the committed tree non-compiling.

`dotnet4` (GAP4 stage 4, .NET metadata) is **not committed**: its module is real but its consumer is dead
wiring. `core/observation_records.rs:116` feeds `text(r,"report").as_bytes()` into a PE locator, yet
`report` is written at `core/observation_records.rs:56` as `s(report)` — the observation tool's prose text,
bounded by `MAX_REPORT` — so it never carries raw PE bytes and `retained_image` returns None on every
product record. The product's real PE path is the `data.starts_with(b"MZ")` branch at
`core/investigation.rs:101-103`, which is where lane `dotnet5` is moving the consumer. The coverage itself
is new and was confirmed as such: the CLI header (data-directory index 14) and CLR metadata are read
nowhere else in `core/` — `pe_observation.rs` covers imports (dir 1), certificate (dir 4) and base
relocations (dir 5) only. Reuse finding, recorded not fixed: `dotnet_evidence` re-implements MZ and
optional-header parsing privately because `pe_observation::Image` (:24) and its reader are not exposed
(only `analyze`/`observe_with_cancel` are `pub(super)`).

Two more GAP items were dispatched after measurement rather than guesswork. `defl5` targets the native
archive writer's real defect: `tools/package.rs:123 deflate_store` emits stored DEFLATE blocks while
`tools/package.py:109` writes real DEFLATE at `compresslevel=9`, so the natively built archive is not a
compressed archive; `core/compression.rs` has `inflate` only, and no code path in `core/` reads data
directory index 14 or emits a deflate stream. `loop7` targets GAP2's remaining proof bar: every field it
needs already exists in the product — `budget_for` (`core/rrsi.rs:685`), `budget_line` (:702, embedding
`pruned_components` :788), `prune_refusal` (:716), `incumbent_cost` (:812), `incumbent_base` (:1622), the
`noise_floor` policy key (:63) and numeric guard (:313), and the held-out leg keys (:70-79 with
`known_leg` :280) — so GAP2 is now an instrument extension of `verification/rrsi_loop.py` over at least
three rounds, not a product gap.

Instrument lesson: `tools/build-units.sh <out>` emits ONE executable FILE at `<out>`; the sanctioned
measurement is `python3 tools/checks.py --all-units <that file>`. A first attempt in this window invoked
`./bin/product-unit-targets` and `./tools/build-units-targets`, which produced exits 126 and 127 and no
measurement at all — a compile-only `BUILD_EXIT:0` is not a case count.

The delivered record (71 groups, 4,215 checks, 67 pass, 0 fail, 4 unavailable) is still backed by a
product binary of mixed provenance (built at 09:23:49 from `e7d13ec` plus `js3`'s then-uncommitted files).
A second full sweep on a settled tree must supersede it before that record is cited as current.

## Worktree provenance lands, GAP2's blocker turns out to be the route, and a lane's own commit adds a dead-code warning — 2026-11-12T20:1Z–22:3Z

`root3` landed as `436b281` (only `core/rrsi.rs` and `core/harness.rs`). It rewrites how a refinement
finds its work tree: `locate_worktree` canonicalizes **before** git runs (so a relative `--git-dir`
resolves from the work tree rather than the caller's spelling), `require_recorded_worktree` compares the
`dev:ino` identity from `workspace_identity` before any git subprocess or `.git/info/exclude` write, and
`validate_fields` gained an exact-match arm for `workspace identity does not match the approved root` so a
mismatch seals **unscored**. Witnessed in a `git archive` room of `44b755e` plus exactly those two files
(md5-identical to the worktree): **686/686 cases plus 55 isolated application cases, exit 0** — HEAD's 672
plus the 14 `#[test]` markers the diff adds, 7 per file. Every added `Command::new("git")` and
`std::env::current_dir()` sits past the `#[cfg(test)]` boundary (`core/rrsi.rs:1873`, calls at
:2393/:2423/:2461; `core/harness.rs:1558`, calls at :2025/:2049/:2054/:2364), so production R1 exposure is
unchanged. The audit seat's ordering leg is closed by census: the identity writer is production
(`core/approval_rules.rs:12`, which computes `dev:ino` and writes `approval_root_identity` before
`approval_root`), both production call sites (`core/harness.rs:437` refine entry, :782 pool path) pass the
recorded identity, and **no** production call site passes an empty one — `require_worktree_root` is reached
only from tests. A decorated identity sentence does not slip past the seal: with scores present the exact
sentence is refused, pinned by `identity_mismatch_seals_without_scores_and_a_longer_sentence_does_not`.

The same room's product build (`bash headless.sh`) caught a regression that commit introduced:
`warning: function require_worktree_root is never used` (`core/rrsi.rs:1592`), one of 26 warnings. The
wrapper was live at `44b755e` — it became dead exactly when the two production call sites switched to the
identity-carrying variant. Kept as an open cleanup rather than papered over, and not dispatched immediately
because `core/rrsi.rs` is owned by the in-flight GAP2 lane.

`loop7` grew `verification/rrsi_loop.py` from 6 to 13 checks and reported, honestly, that three of GAP2's
six sub-claims are **not produced** by the route: no noise-floor sentence, no pruned component, no held-out
keys. Its audit seat confirmed 6 of the 7 new checks are non-vacuous and that checks 5 and 7 lean on absence
assertions (`Pruned:`, `held_out_*`) that are green by construction while no production writer exists —
valid as regression guards, not as capability witnesses. Coordinator legs closed on the group: it exits
`raise SystemExit(1)` on failure (`:455`), runs inside a per-run `TemporaryDirectory(prefix='rrsi-loop-',
dir=PROJECT/'output')` (`:78`) with the product data home inside it (`:82`), and launches
`PROJECT/'output/headless/RLLM'` (`:106`) with no freshness pin, so the binary must be dated before the
witness is cited. Its commit is **held** because the GAP2 lane is editing that same file — committing now
could capture a half-finished edit.

That corrects the previous section's closing claim that "GAP2 is now an instrument extension of
`verification/rrsi_loop.py` over at least three rounds, not a product gap". Measured: it is a **route** gap.
`core/improvement.rs:113-119 refinement_rounds` accepts only `1` or `2` and answers anything else with
`Select one or up to two refinement rounds`, so a real run records round ids `{1,2}` only and the annealed
budget reaches 4 then 3, never `b_min`. The RRSI machinery is already live and unit-tested —
`annealed_budget` (`core/rrsi.rs:684`) and `budget_for` (:687, used at :705 and :1397),
`acceptance(baseline,candidate,floor,cost_before,cost_after)` (`core/improvement.rs:129`),
`prune_refusal` (:718) and `pruned_components` (:790, consumed at :707/:805/:895/:1403), leakage values
validated at :180 — but the held-out leg has no production writer: `core/harness.rs:922` reads the run-state
`held_out_known`, and the only writers of that key (`:2529`/:2531/:2571) are inside `#[cfg(test)] mod checks`
(opened at :1562). `noise_floor` was listed here as having no production writer; **that was wrong and is
corrected** — `core/harness.rs:935` writes the acceptance-policy floor and `:123` copies it into the rrsi edit
row (`core/rrsi.rs:315-316` validates it, and `:2236` is only rrsi's own test). What is still owed is a *route*
witness: no journal on disk carries a `noise_floor` row today (`grep -rl noise_floor output/` matches only
binaries). So the open item is the ceiling plus a production writer for the held-out leg, not the acceptance
machinery.

`dotnet5` relocated the .NET/CIL reader off the dead consumer and onto the real `MZ` branch:
`cli_evidence` (`core/dotnet_evidence.rs:1017`, outside `#[cfg(test)]`; the JSON view `subject_view` at
:1010 is test-only) is now called from `core/investigation.rs` inside `data.starts_with(b"MZ")` after the PE
observation and a `cancelled()` check, and the dead `dotnet_image`/`retained_image` line is gone
(`core/observation_records.rs` no longer appears in the diff). Verified by the coordinator in a
`git archive` room of `436b281` plus its three files: **701/701 cases plus 55 isolated application cases,
exit 0** — 686 plus 15 (the module's 7 pre-existing untracked cases plus its 8 new ones). The wiring is
load-bearing and the coordinator measured it: deleting **only** the consumer line in that room gives
**698/701, exit 1**, and the three cases that go red are
`cli_directory_appends_exact_dotnet_lines`, `inconsistent_cli_extent_is_refused_by_name_in_inspect` and
`truncated_metadata_stream_names_the_unread_header_in_inspect`, panicking at `core/investigation.rs:444`/:459/:490.
`pe_without_cli_directory_keeps_inspect_text` survives that mutant — it is a preservation guard, not a
wiring witness — exactly as the seat predicted. Still open from that seat: the `Some(String::new())`
substitution mutant, and the absence of any aggregate output/work cap charged against the inspect budget for
a large PE carrying directory 14 (the visible caps are the metadata table cap, the metadata stream cap and
the method row cap), with no cancellation hook inside the CLI parser.

The second `leg()` exception-swallow instance is now censused: `if name not in evaluated` appears at
`verification/rrsi_loop.py:77/:405/:440` and `verification/self_rebuild.py:162/:276` — five sites in two
files, no others. A `selfreb` lane owns `self_rebuild.py`; `rrsi_loop.py`'s copy rides with the GAP2 lane.
A fifth lane targets GAP4 stage 2's dead typed surface: `Observation.xrefs` (`core/control_flow.rs:6`) is
computed at :294 and printed at :295 but never read as a field (`warning: field xrefs is never read`), and
`binary_declarations::derive` (`core/binary_declarations.rs:45`) emits only mappings, classifications and
relocations even though that projection is production-live (`core/binary_evidence.rs:50`/:81,
`core/reconstruction.rs:12`) — so cross-reference edges reach the prose but never the typed graph.

`defl5` closed GAP5's first half — the archive compressor. `deflate` (`core/compression.rs:644`, outside
`#[cfg(test)]`) writes RFC1951 raw DEFLATE in fixed **or dynamic** Huffman blocks — it emits a dynamic block
only when that is smaller, which is why the corpus result sits near zlib level 6/9 — with a hash-chain LZ77
match finder run per 64 KiB chunk, so a match never crosses a chunk boundary and the effective lookback near
a chunk start is under the 32 KiB window; the chain walk is capped at 4096 steps (128 after a length-32 match)
and stops at a 258-byte match, so worst-case work per chunk is bounded but superlinear (≈65,536 lookups ×
4,096 candidates), not a linear-time guarantee. A block that would grow falls back to STORED with correct
`LEN`/`NLEN` (`NLEN = 0xffff − LEN`) and ≤5 bytes of overhead per ≤65,535-byte chunk, and `deflate` returns
the stored form unless the compressed form is strictly smaller. `tools/package.rs`'s `write_zip` now calls the
tool's own copy of it (the tool is a standalone binary that already carried its own inflater) instead of
storing every member. Verified in a `git archive` room of `28b29d3` plus the lane's two files: **708/708 cases
plus 55 isolated application cases, exit 0** — 701 + 7, matching the lane's own new-case count — and the
package tool's suite is **29/29**, up from 20, exit 0. The lane's first delivery was refused for two defects
found by measurement, not review: it had written the compressor to a new `core/deflate.rs` with **zero markers
and no `mod` line** (dead code that would not even compile, and would have needed two interlock edits), and
its archive test asserted a hand-computed header offset that was wrong by two bytes —
`fnv(&archive[..payload - 6..])` covered the extra-field length field and the CRC, so the pinned digest
refused it while the comment claimed the whole member prefix.

The audit seat for this commit found no Critical defect and two Important wording defects, both corrected
above: the encoder is not fixed-block-only, and the zlib conformance evidence is a **coordinator probe**, not
a test inside the patch — the patch's own legs round-trip through the project's inflater. The seat also
checked which legs survive a stored-only `deflate`: `deflate_growth_uses_stored_blocks`,
`archive_writer_stores_a_growth_member`, `zip_roundtrip_verifies_crc_and_length`, `truncated_deflate_stream_is_refused`
and `length_limited_codes_stay_complete` do, while `deflate_empty_is_fixed_block`,
`deflate_short_literals_match_fixed_block`, `deflate_long_run_is_dynamic_and_smaller`,
`deflate_roundtrip_at_stored_block_boundary` and `archive_writer_shrinks_a_compressible_member` all fail, so
the suite is not vacuous. Both archive legs assume a local header with no extra field, which the writer's own
code satisfies.

Conformance was measured without the product's own inflater, so the claim does not rest on a self-consistent
pair: a standalone probe compiled against `core/compression.rs` deflates a 230,800-byte corpus
(repetitive text, random bytes, a zero run, and `/etc/hostname` + `/etc/os-release`) to 21,519 bytes, and
python zlib's raw-DEFLATE decoder returns the exact original; two runs are byte-identical; and the project's
native inflater accepts a zlib-9 stream byte-identical. Size against zlib level 9 on the same corpus is
4.98 % over (`21,519` vs `20,500`), 0.14 % under level 6 — near parity, **not** byte parity, so the archive
bytes change and no pinned archive digest may be claimed from this. The Python packager is therefore still
not retired: `tools/package.py` remains until a real publication archive is produced and compared against the
Python one, which is still blocked behind the packager-publish blocker (`output/checks/apk.json`).

`selfreb` fixed the `leg()` swallow in `verification/self_rebuild.py` (`fb2adc9`): a leg that raises after
its own check has been recorded now lands in `problems`, so a raising leg is a failure. Group checks 8 → 9,
and its record shape was corrected against the runner source rather than guessed: `tools/harness.rs:577-594`
watches only `count`, `passed` and `checks` (`failed`/`scope` are skipped) and a group passes only on exit
code 0, while `Measure::finish` (`:362-378`) compares **only the watched fields that are present** — so the
green line sets `passed` to the recorded count and a red record omits `passed` rather than publishing `0`
against `count = 9`, which would have become a `Conflict` → `unparsed` and blocked the publish. Its group was
then re-run on the committed bytes and is **green: 9 checks, exit 0** (record `output/self-rebuild.json`).
An earlier note in this window said that group went red because the product could not build — **that was
wrong**: a `git archive HEAD` room built `output/headless/RLLM` (12,534,832 bytes) with `headless.sh` exit 0
and 64 warnings, so the dead-code warnings are a wiring signal, not a build blocker. The warnings that map
onto open work are `field xrefs` (GAP4's typed graph, a lane owns it),
`durable_successor_rows`/`successor_states`/`job_source` (GAP1 read-back, a lane owns it),
`leg_score`/`score_split`/`SPLIT_CASE_MAX` (GAP2's held-out rows, the GAP2 lane's target) and
`require_worktree_root` (`core/rrsi.rs`, orphaned by root3's guard swap). `verification/rrsi_loop.py`'s copy
of the same fix is written and audited but deliberately uncommitted: that file is being edited by the GAP2
lane.

Open now, beyond the standing owner-gated items (the signed companion APK triple, `PUB-INPUT-DRIFT`, the
`binary_flow`/`history_migration` comparators, `DESKTOP-SURFACE`, the memory index):

- GAP1 (wire the successor-state read-back and a real handoff chain): **stage 1 landed in `f28e463`** — the
  handoff now reads and validates the companion chain before exec, and `durable_successor_rows`/
  `successor_states`/`job_source`/`track` are no longer dead code (64 → 60 warnings). **Stage 2 landed in
  `c396323`** — the view observes and stores the exit instead of relying on `std::process::Child` caching it, so a
  dead-but-unreaped PID no longer reads `running = true`; the missing-companion refusal fires only when this process
  actually tracked that companion, and the tamper probe proves the row digest covers the payload. Still open: an
  absent companion journal in a *fresh* process is still indistinguishable from a deleted one (the tracked table is
  process memory and `exec` discards it); the chain resists reordering and insertion but not truncation to an
  earlier valid prefix, replacement or rollback (lane `g1s4`); a signal-killed successor renders `exit: null` in the
  view while the durable row carries the signal; and no connected verification group exercises the handoff chain
  yet, so the delivered record still covers none of it.
- GAP2 (raise the `refinement_rounds` ceiling with a real 3+-round run to `b_min`, and give `noise_floor` and
  the `held_out_*` keys production writers): a lane owns `core/improvement.rs`, `core/rrsi.rs`,
  `core/harness.rs` and `verification/rrsi_loop.py`, including the two `== 2` equality gates at
  `core/harness.rs:967`/:1053 and the pinned ceiling assertion at `verification/rrsi_loop.py:381-383`.
- GAP3 depth (the improvement-library index `library_index` (`core/rrsi.rs:1202`) is now *produced and stored* —
  `966580b` copies the resume receipt's index onto `task_run` — but it still has **no product reader**, so no round
  consumes it (lane `g3c`);
  probe-scored and composed candidates; chained sequential PPC for `k > 1`; agentic selection; the
  non-atomic `apply_accepted` swap), plus removing `root3`'s orphaned `require_worktree_root`.
- GAP4 (the white-room rebuild end-to-end, `.NET` depth/caps/cancellation, and the typed cross-reference
  group that GAP4's own report names as "calls and cross-references"): the white-room kernel is wired to disk
  (`0b1cf39`), the `.NET` parser carries three named caps with a live consumer in `inspect` (`51ce540`), and the
  typed declaration graph landed (`b6457b0`). The remaining named hole is the container: HAR is read and wired
  (`core/har_evidence.rs` → `core/observation_records.rs:115`), but a `.saz` archive is not recognised at all —
  lane `saz1` owns that, reusing `core/archive.rs`'s bounded `unpack` rather than a second zip reader.
- GAP5 (retirement of the Python instrument roles, in the recorded order): the deflater is now the
  product's own (`2b041bc`), and since `56e1dbb` the **rule itself has a native gate** — the publisher prints and
  enforces a Python census (`core` 0/0, `platform` 0/0, `tools` 2/189 allowlisted with named roles, `verification`
  capped at 73 files / 8,570 lines and only shrinkable, anything else refuses). Still to retire is
  `tools/checks.py`'s sweep/units-replay role, then `tools/package.py` once a real publication archive is compared
  against the Python one. The 73/8,570 debt is instrument debt, not product debt, but it is now loud: adding a
  `verification/*.py` file or growing one refuses `publish`.

A running lane also reverted this document's uncommitted edits once during the window (`git stash list` empty,
no conflict — a writer with `--always-approve` inside the main worktree can restore tracked files). The
deflater and self-rebuild paragraphs above were re-applied after that; keeping the record current means
committing documentation promptly rather than leaving it staged.

The delivered record is unchanged this window: 71 groups, 4,215 checks, 67 pass, 4 unavailable, still backed
by the 09:23:49 mixed-provenance binary. A single sweep on a settled tree must supersede it, and that sweep
is gated behind five in-flight lanes plus the signed-APK inputs that keep `apk_alignment` and
`turn_background` unavailable.

## A white-room rebuild lands, cross-references become typed rows, and a git-archive room turns out to be the wrong instrument for a sweep — 2026-11-12T22:3Z–2026-11-13T01:0Z

`room6` landed the GAP4 white-room rebuild as `06d1f6b` (`core/clean_room.rs` new, `core/file_evidence.rs`,
`core/reconstruction.rs`). A `clean_room` mode rides the existing `reconstruct` route: `reconstruction::validate`
accepts the mode, `run` requires a complete unchanged file and dispatches to `file_evidence::reconstruct`, which
retains the subject only under the named 128 KiB cap, builds a citation bound to `dev`/`ino`/`size`/`sha256` and
the record/snapshot/node **before** generation, generates a skeleton from the parsed observer items
(`core/clean_room.rs:379`), refuses a byte-identical result as `clean-room copy` (`:65`), creates the output with
`openat(O_CREAT|O_EXCL|O_NOFOLLOW)` relative to the held directory fd, and reads the written bytes back chunk by
chunk. Caps are named (`clean-room item cap`, `spec-size cap`, `generated-byte cap`, `subject-byte cap`).
Coordinator witness: a `git archive` room of HEAD plus those three files, md5-identical to the worktree, measures
**718/718 unit cases plus 55 isolated application cases, exit 0** (708 + 10). Two mutants in that room each go
red for exactly their own reason: deleting only the `clean_room` dispatch from `reconstruction::run` gives
**717/718** with `application::file_evidence::checks::clean_room_route_cites_generates_and_verifies` panicking at
`core/file_evidence.rs:133`, and neutering the copy refusal at `core/clean_room.rs:65` gives **717/718** with
`application::file_evidence::clean_room::checks::byte_identical_artifact_is_refused_as_a_copy` red. Restoring both
files reproduces the original md5s. The audit seat found no Critical defect and three real gaps, all open:
`clean_room::verify_artifact` (`:468`) has **no production caller**, so re-observation runs over the generator's
buffer while the disk read-back is only compared byte-for-byte; `byte_identical_artifact_is_refused_as_a_copy`
drives the `reject_identical` helper directly rather than the route, so it cannot convict a generator that copies;
and the item/spec caps are checked *after* `observe_items`, so the observer's own intermediate output is bounded
only by the 128 KiB subject cap.

`natx` landed GAP4 stage 2's typed graph as `b6457b0` (`core/binary_declarations.rs`, `core/control_flow.rs`).
`derive` now emits a **fourth** group, `cross_references`, with `observed`/`retained`/`omitted`/`rows`, and the
document carries `cross_reference_inventory_complete`; `Observation.xrefs` is read in the product at
`core/binary_declarations.rs:62`/:66, which is what clears `field xrefs is never read`, and `Observation` gained
`xrefs_complete` — `Some(false)` when the pass is withheld or an edge/unknown cap fired, `Some(true)` only when the
stored edges and unknowns are the whole record, `None` when the pass never ran. A resolved PE target above
`2**32` is refused by name while the same wide target stays valid for `ELF declared VA`. Witness: the same room
with these two files overlaid measures **723/723 plus 55, exit 0** (+5 cases). The seat found no Critical defect;
its Important limitation predates the lane (the projection materialises rows and only then compares `state_bytes`
with the budget, so the document shell is never pre-reserved), and it flagged one Minor inconsistency that is now
an open item — the emitted `meaning` sentence still names only mappings, labels and relocations although a fourth
group is emitted.

**Instrument finding, measured:** a `git archive` room is the wrong instrument for the connected sweep. A full
sweep inside such a room (tools, product and runner all built inside it) reports **63 pass, 1 fail, 7 unavailable
of 71 groups** and publishes nothing. Five groups are `UNAVAILABLE` there that pass on this machine —
`binary_flow`, `history_migration`, `field_flow`, `flags_flow`, `product_flow` — because their declared inputs are
`output/*-controller` files, and `output/` is gitignored so the room never had them; the two APK-gated groups stay
unavailable as before. The single FAIL is `verification/rrsi_loop.py`'s leg
`refine entry copies approval_root onto task_run.workspace and records source_digest` (exit 1, 6 checks,
reproduced standalone in about a second), and it is a substrate artifact, not a product red: an archive room has no
`.git`, so `require_recorded_worktree` (root3, `436b281`) falls back to the raw `approval_root` and
`source_digest` is never recorded — the leg expects it. Consequence recorded for the pending sweep: the delivered
record must be produced in a tree with real git state (the main tree, or a `git worktree`), not an archive room.

The delivered record is unchanged this window: 71 groups, 4,215 checks, 67 pass, 4 unavailable, still backed
by the 09:23:49 mixed-provenance binary. A single sweep on a settled tree must supersede it, and that sweep
is gated behind the four in-flight lanes plus the signed-APK inputs that keep `apk_alignment` and
`turn_background` unavailable.

## GAP1's handoff reads its successor chain off disk, my own mutation claim turns out to be wrong, and the audit seat's prediction beats it — 2026-11-13T01:0Z–03:3Z (program clock; host clock 2026-11-12T04:0Z–11:0Z local, `f28e463` committed at 11:01:57-05:00)

**GAP1 stage 1 landed as `f28e463`** (`core/self_build.rs`, +241/−18). `successor_witness`
(`core/self_build.rs:975-983`) reads the companion journal beside the handoff's `journal` field through the
pre-existing `durable_successor_rows` (`:1419-1440`), which parses **every** line, refuses a row whose `revision`
is not `sha256(without_revision(row))` (`successor journal row is not content-addressed`) and a row whose `parent`
is not the previous row's revision (`successor journal parent does not match the previous row`). `handoff` emits
`successor_rows`, `successor_tip` (disk-derived) and the live table `successors` at `:1012-1019`, **before** the
`review_confirmed` exec arm at `:1021+`, so an invalid companion journal is refused rather than exec'd.
`job_source` is now carried into the tracked-successor table (`:1632-1635`), which is what clears the GAP1 dead-code
trio: the product build's warning count went **64 → 60** (main tree at `b6457b0` versus the room's own product
build, `/tmp/gap1bv-product.log`). Witness: room `/tmp/gap1bv` = `git archive b6457b0` plus **only** this file
(the other four modified core files in the worktree verified byte-identical to `b6457b0`) measures
**729/729 plus 55 isolated application cases, exit 0**, and the mutant binary measures 726/729.

**A claim of mine was wrong and is corrected.** My in-progress note said removing only the successor read turns
"5 of 6" new tests red. Measured, with the read replaced by `Ok((0, String::new()))` and the log fields and exec arm
kept, the answer is **3 of 6**: `handoff_reports_the_tip_of_a_valid_two_row_successor_chain`,
`handoff_refuses_a_successor_row_whose_revision_is_not_the_body_digest`,
`handoff_refuses_a_successor_parent_that_misses_the_previous_row`. The three that survive are
`handoff_reads_a_missing_successor_journal_as_an_empty_witness` (it expects zero rows, so an always-zero mutant
satisfies it), `handoff_successor_view_separates_a_running_spawn_from_a_reaped_exit` (live table only), and — the
one that matters — `handoff_review_confirmed_execs_the_binary_after_a_valid_chain`: its child writes a proof file
and the parent asserts the proof, **but the proof is still written when the disk read is deleted**, so that test
does not prove exec is gated on a validated chain. That is a test defect, not a documentation slip, so writer lane
`xect` is dispatched to add a tampered-chain leg with its own proof-file name — refused child, no proof file,
refusal sentence asserted — and to prove that leg red under the same mutant.

**Audit seat (MiniMax-M3, diff inlined because the seat has no file access).** First pass refused to credit
anything it could not see and marked six claims NOT SUPPORTED; that was correct behaviour, since
`durable_successor_rows`, `successor_states` and `reap` are pre-existing code and therefore absent from the diff.
Sent their HEAD bodies, it concluded: every element of the returned vector passed both the digest and parent-chain
checks; a torn or partial final line **aborts the read** rather than being skipped, so `rows.last()` is a validated
tip; the tip is disk-only and cannot be overridden by a stale in-process entry; **no Critical or High finding in the
new code**. Open items it produced, all recorded rather than fixed here: an absent companion journal reads as the
empty witness, which cannot distinguish "never existed" from "deleted, renamed or mislocated" — a formerly durable
chain that vanishes reports *no successor* instead of failing; the parent chain buys anti-reordering,
anti-omission and anti-insertion inside a chain, **not** protection against truncation to an earlier valid prefix,
wholesale replacement, deletion or rollback; and a tracked PID that has died but has not been reaped still reports
`running = true` until `reap()` runs.

**Lanes in flight at 03:3Z:** `dotnet6` (`core/dotnet_evidence.rs` +1,020, `core/investigation.rs` — inspect-budget
bound, in-parser cancellation, missing metadata/CIL/offset coverage), `room7` (`core/clean_room.rs`,
`core/file_evidence.rs` — the three open `06d1f6b` findings: `verify_artifact` with no production caller, the
copy-refusal test driving the helper instead of the route, caps checked after `observe_items`), `gap4v` (a connected
verification leg for the clean-room route; `verification/product_flow.py` touched), `gap2e` (`core/improvement.rs`,
`core/harness.rs`, `verification/rrsi_loop.py` — three-round refinement ceiling with a production `round_index`
writer and a `held_out` phase), and `xect`. None of their work is committed; `verification/rrsi_loop.py` (+225,
from the finished `rrsik` lane) is still awaiting adjudication, and its leg list is still **six** names, so no
three-round loop witness exists yet.

## Three lanes land, three audit seats hold two of them, and the provenance guard turns out to refuse real git worktrees — 2026-11-13T03:3Z–06:0Z

**The committed tree is green on its own.** A real `git worktree` at `dbfa7db` (not an archive room) built its own
units binary and measured **729/729 cases plus 55 isolated application cases, 0 failures**. Nothing in the commit
chain depends on a lane's uncommitted file.

**`gap4v` landed as `2c3f939`** (`verification/product_flow.py`, +13/−1): the clean-room route now has a connected
leg. Differential witness, main tree, product binary dated `2026-11-12T10:26:49`: HEAD **30 checks / 30 passed /
exit 0**, with the file **36 / 36 / exit 0**. The seat confirmed the legs drive the real route
(`inspect_file` → `reconstruction_prepare` → `approve` → tool) and that the exclusive-create leg proves a named
error (`Cannot create new streaming output: File exists (os error 17)`) plus unchanged bytes. Two legs are weaker
than their names, and that is recorded rather than glossed: `clean-room generated file exists and its bytes differ
from the reference` compares the generated file against the **input file** rather than the retained subject bytes,
and the incomplete-observation leg pins a state **notice** (`Clean-room reconstruction requires a complete
unchanged observed file; missing or prefix content is not reconstructed`) instead of a tool refusal. The report
check is substring-based (`'Spec items:'`, `'Verification matched'`), so a generation that implements nothing could
still pass. Lane `g4v2` is dispatched to strengthen all three.

**Room witnesses for the two Rust lanes.** `git archive dbfa7db` plus `core/improvement.rs`, `core/harness.rs`,
`verification/rrsi_loop.py` (lane `gap2e`) measures **731/731 plus 55** (+2 cases). Plus
`core/dotnet_evidence.rs`, `core/investigation.rs` (lane `dotnet6`) measures **739/739 plus 55** (+10 cases).
Both lanes are held: each audit seat found claim-vs-code mismatches.

**`dotnet6` — three High findings, so the commit is held and lane `dn7` is dispatched to close them.** The new
caps charge *declared* sizes rather than work actually touched (`fill_names` charges `strings_row.size`,
`read_table_stream` charges `table.size`), so an image that merely declares a huge stream is refused without being
scanned, while `examine`'s CLR-header/metadata mapping is not charged at all (`map_range` charges only the
section-window iterations). The output cap bounds the assembled report, not the intermediate accumulation
(`heap_lines`, `method_headers` are built before `Sink::line` sees them), so claim 1's "cannot emit unbounded text"
is only true of the final string. And `partial_methods` (up to 64 method rows, reached from error paths) is outside
both the work budget and the cancellation token, which is the strongest escape. Cap disclosure itself is real
(`dotnet observation byte cap`, `dotnet parse work cap`, `WORK_UNREAD`), and the seat's R-rules verdict on the
production diff is PASS.

**`gap2e` — one Critical, and the investigation it triggered turned into a bigger finding than the lane.** The
seat flagged that the refine arm now records `source_digest` when `require_recorded_worktree` fails for any reason
other than an identity mismatch, and that the consumer at `core/harness.rs:860` (`if !text(run,"source_digest").is_empty()
{…} else { recorded_source(root)}`) then skips `recorded_source`. I chased why that arm was needed at all, and my
first explanation — that `locate_worktree` requires `<root>/.git` to be a **directory** while a linked worktree
stores `.git` as a file — is **true as a fact but was not the cause**, so it is corrected here. Measured chain:

- The group plants its own git store in the fixture room (`plant_git`, `verification/rrsi_loop.py:44-53`) inside
  `PROJECT/output` (`:78`), with an isolated `XDG_DATA_HOME` under the room (`:80-85`). Probed directly, that planted
  store satisfies git: `rev-parse --is-inside-work-tree` → `true`, `rev-parse --show-toplevel` → the room,
  `rev-parse HEAD` → a commit hex, `worktree list` → the room. So the fixture is a legitimate work tree by git's
  own answer, and the directory-vs-file rule never fires for it.
- The leg is red in **every** tree I built: the main tree (`/tmp/rrsi-maintree.out`, exit 1, one of six legs), a
  linked worktree at `dbfa7db` with the product built inside it (`/tmp/rrsi-head-gitreal2.out`), and the same
  worktree with my scratch `.git`-file experiment applied (`/tmp/rrsi-wtexp.out` — still the same single leg). The
  failure detail is identical everywhere: `source_digest=None expected=absent`, with `approval_root` and `workspace`
  both equal to the canonical room.
- The delivered record's green `rrsi_loop` row (`verification/RESULTS.json` published 09:26:20,
  `connected.counts.rrsi_loop = 6`, 67 passed of 71 groups) comes from a binary built **before** `436b281`
  (10:04:29, "Bind refine and pool adoption to the approved work-tree identity"). So this leg has been red against
  every product built since `436b281`, and the record still carries the pre-guard green row. That is a real
  unowned regression, exactly the mixed-provenance hazard this file keeps warning about.
- The refusal reason is **invisible** in the product: the refine arm's failure path (`Err(_)=>{put workspace}`)
  swallows the sentence, so the group can only observe a missing `source_digest`. Finding which arm fires —
  identity mismatch, `ignore_worktrees`, or something else — is now an explicit work item, not a guess.

Lane `gap2e`'s arm makes the group green again by recording the pre-existing `SOURCE_ABSENT` marker, and the group's
own model already treats a missing `rllm-harness.txt` as `"absent"`, so the sentinel is not invented. But it records
provenance for a root whose work-tree check failed, which is what the seat called out, and it hides the regression
instead of fixing it. Adjudication: the arm is dropped, `core/rrsi.rs` and the swallowed refusal become a work item,
and the leg must go green in the main tree without weakening provenance. `locate_worktree`'s linked-worktree gap
stays a separate real finding: it is what stops any git-reading group from passing in a linked worktree, so the
pending delivered sweep must run in the primary tree.

## Two lanes land, the loop regression gets a 2×2 attribution, and I reverse my own adjudication from the last block — 2026-11-13T06:0Z–08:0Z

**The unit line is re-pinned at 737.** A room holding only `git archive f1051da` (no lane work at all) built its units
binary and measured **737/737 cases plus 55 isolated application cases, exit 0**, which matches the chain arithmetic
(`729 → +6 in 0b1cf39 → 735 → +2 in f1051da → 737`).

**`room7` landed as `0b1cf39`** (clean-room). The route used to verify the created file against the generated buffer
and then report counts from an in-memory re-render; it now accumulates the read-back into a buffer, runs
`verify_artifact` on *that* buffer, refuses if the disk observation disagrees, and builds the entire response from it,
naming the length: `Verification matched 5, missing 0, unknown 0 on the N bytes read back from disk`. Room witness
(`git archive 3a4550f` + the two files, md5 `0c0430b5c43385b5` / `79f67cc1eb4a82ba`): **735/735 plus 55, exit 0**.
Mutation: reporting the in-memory counts again reddens
`application::file_evidence::checks::clean_room_route_reports_counts_from_the_bytes_read_back`.
The seat raised three P1s and **two of them are artifacts of my own truncated diff** — I sliced it at 17,012 bytes, so
the seat could not see the final `format!` (which does use the disk observation) or the route-level cap test
(`clean_room_route_refuses_an_over_cap_observation_without_writing`, which drives `reconstruction::run` and asserts no
output file). Its third P1 — items and gaps each bounded by `SPEC_ITEM_CAP` separately, so the total admitted records
can reach twice the cap — is real but is a bound on *parse work*, not a promised single-record count; I also measured
that `parse_fact` yields an item only for the five fields the pre-parse check names, so the lane's field list is exact,
not an omission.

**`gap2e` landed as `f1051da`** (three refinement rounds, a `held_out` round, and the group that proves them). Room
witness (`git archive 475e34c` + `core/harness.rs`, `core/improvement.rs`, `verification/rrsi_loop.py`):
**731/731 plus 55, exit 0**, and the connected group **`rrsi_loop PASS (exit 0, 1s, 13 checks)`**. Mutation of the new
production writer (removing `put(&mut run,"round_index",Num((index+1).to_string()));`) gives **730/731** with exactly
one red: `application::harness::checks::three_refinement_rounds_record_budget_floor_leakage_prune_held_out_and_commit`.
Before this change the ceiling was unreachable: `complete_refinement_round` compared `refinement_rounds == 2` and
nothing ever wrote a round index.

**I reverse my previous adjudication about that group.** Last block I wrote that `gap2e`'s arm "hides the regression
instead of fixing it" and dropped it. Measuring the 2×2 says otherwise:

| product | group file | `rrsi_loop` |
|---|---|---|
| `44b755e` (pre-guard) | committed (6 checks) | **PASS** |
| `44b755e` (pre-guard) | lane's new file (13 checks) | FAIL — the three-round legs need `f1051da` |
| `dbfa7db` (guarded) | committed (6 checks) | FAIL — `refine entry copies approval_root onto task_run.workspace and records source_digest`, fixture prints `source_digest=None expected=absent` |
| `475e34c` + `gap2e` | lane's new file (13 checks) | **PASS, 13 checks** |

So the red was introduced by `436b281` (the guard) *and* the new legs need the three-round product change; neither
half is sufficient alone. The seat's Critical — "provenance recorded on a failed work-tree check, and the consumer
skips `recorded_source`" — does **not** put the pool on an unverified tree: `evaluate_pool` calls
`require_recorded_worktree` again at adoption time (`core/harness.rs`), so refusal of the pool is independent of the
digest recorded at refine time. What is still not pinned is **which** guard arm fires for that fixture room: with
`gap2e`'s arm the leg goes green, and that arm records the digest for every error *except*
`workspace identity does not match the approved root`, so the fixture's failure is some other arm — probably
`locate_worktree`/`finish_worktree` rejecting the hand-planted store. The refusal sentence is still swallowed in the
shipped binary, and that remains a work item. `locate_worktree`'s linked-worktree rule (`.git` must be a real
directory) stays a separate, real finding: any git-reading group must run in the primary tree.

**Held on findings that survived my own check.**
- `core/self_build.rs` (lane `g1s2` plus lane `xect`'s tamper tests) is green in an isolated room —
  `git archive f1051da` + that file gives **741/741 plus 55, exit 0**, warnings unchanged at 13 — and both new
  refusals are load-bearing: removing the vanished-companion refusal gives **740/741** red on
  `self_build::checks::handoff_refuses_a_vanished_successor_journal_beside_a_journal_with_rows`. Two P1s keep it
  uncommitted: the truthful `running`/`exit` view depends on `Child::try_wait` caching the status inside `Child`,
  which is a current-implementation detail and not the documented contract; and the vanished-companion predicate is
  exactly *journal has ≥1 row ∧ companion absent*, which cannot tell "vanished" from "this handoff never tracked a
  successor" — a first real handoff with an ordinary non-empty journal would now refuse. That hazard is latent, not
  shipped: `self_handoff` is dispatched only at `core/lib.rs:443` and `grep -rn self_handoff platform/ gui/
  verification/` returns nothing, so no surface calls it yet. Lane `g1s3` owns both fixes.
- `core/rrsi.rs` (lane `rdx`) emits `library_index` on `resume_run`'s receipt and adds a line-cap refusal, and is
  green alone (`git archive 475e34c` + that file: **731/731 plus 55**), but `resume_run` has **no production call
  site** — `grep -rn "resume_run(" core/` finds only unit tests, and the route rebuilds from
  `records_of(&text(run,"edit_ledger"))` instead. A field on an unreachable receipt is not a capability, and the new
  `load_resume_library` gives a second library loader with a different refusal set. Its cap test also cannot fail for
  the right reason: the at-cap leg asserts `assert_ne!(error, "resume library index exceeds its line cap; ...")`,
  which also passes when resume fails for an unrelated reason, so acceptance at exactly `LINE_CAP` is unproven.
  Lane `rr1` owns reachability plus that boundary.

**Lanes in flight at 08:0Z:** `dn7` (the three `.NET` bound/cancellation findings from `dotnet6`), `rr1`
(`resume_run` reachability), `g1s3` (the two successor findings). Rooms: `/tmp/headw` (real worktree at `dbfa7db`),
`/tmp/preguard` (`44b755e` archive, used for the pre-guard cells above), `/tmp/g2x` (`475e34c` + `gap2e`, mutated and
restored), `/tmp/sb2w` (`f1051da` + `self_build`), `/tmp/rdxw`, `/tmp/room7w`, `/tmp/g2w`, `/tmp/dn6w`, `/tmp/xectw`,
`/tmp/wtexp`. Nothing in the commit chain depends on a lane's uncommitted file.

## The .NET parser lands with three named caps after two seats over-called it — 2026-11-13T08:0Z–08:4Z

`core/dotnet_evidence.rs` + the `MZ` call site in `core/investigation.rs` are committed as `51ce540` (lanes
`dotnet6` then `dn7`, the second closing three High findings the first seat raised). Isolated-room witness
(`git archive 8eff3b5` + those two files, md5 `272d503e98e25a89` / `a43c75754bf17cbf`): **753/753 cases plus 55
isolated application cases, exit 0**, warning count unchanged at 13 — consistent with the chain (`737 → +16 in
51ce540 → 753`).

What is now bounded: the parse budget charges **bytes actually mapped or read**, so a 700-byte image declaring
4026531840-byte streams parses instead of being refused, while a declared range that is fully present and over the cap
is refused under its own name (`dotnet declared size cap`); emitted text is charged line by line as it is produced with
the omission notice pre-charged inside the 16 KiB `dotnet observation byte cap`; `partial_methods` takes the same
budget, charges a row stride per row, is bounded by `METHOD_ROW_CAP`, and its cancellation surfaces as the existing
`Inspection cancelled`. The consumer is no longer dead wiring: `cli_evidence` returns `Absent`/`Observed`/`Bounded`
and the `MZ` branch of `inspect` consumes all three, with tests driving `inspect` rather than the helper.

**Three of the two seats' P1s are refuted against the code, and one of them is a claim I could only catch by reading
the file myself:** the omission notice *is* pre-charged on every accepted line (`charge_observation` adds
`OBSERVATION_NOTICE.len()` to every charge) and a test pins `text.len() <= OBSERVATION_BYTE_CAP`; `sections_of` takes
the same `Budget` and names its own truncation, so section probing is not unbudgeted work; and the pair of
`matches("Refusal: dotnet observation byte cap.\\n")` / full-sentence assertions are **not** duplicate counts — the
patterns differ by the newline, so the short one matches only the notice's first line. Accepted residuals, recorded
rather than fixed: the fat-header path charges a prefix twice (conservative, can refuse near the limit), per-method
header mapping is charged as a row stride rather than per byte (worst case bounded by `METHOD_ROW_CAP` 4096), a
declared size over the cap that is *not* fully present is parsed as the available prefix with no notice, and the
64-iteration table loop and 4-heap loop do not poll cancellation.

## The resume index lands, the publisher gains a Python gate, and the tools room turns out to be an invalid instrument — 2026-11-13T08:4Z–11:2Z (program clock; host clock 2026-11-12T11:1Z–12:2Z local)

**`rr1` landed as `966580b`** (`core/rrsi.rs`, `core/harness.rs`). GAP3's "the improvement-library index is dead"
is now only half true, and the half that was wrong is mine: `resume_run` **did** have a production call site at
`core/harness.rs:253` — what was dead was the receipt. `resume_durable_ledger` called it, copied digest, ledger and
path, and threw the rest away (`.map(|_|())` on one arm, `?` on the other), so `library_index`
(`core/rrsi.rs:1202`) built rows nothing consumed. The route now copies `library_index` onto `task_run`. The lane's
second library loader (`load_resume_library`, held from `rdx`) is gone: `resume_run` and `historical_section` both
read `experience.jsonl` through `load_experience` → `parse_library`, so there is one file, one `LINE_CAP` of 64 and
one cap sentence, and both wrap failures through `library_error`. Room witness `git archive 8de6fd0` plus those two
files: build exit 0, **757/757 plus 55 isolated**, checks exit 0, `warning: 12 warnings emitted` (unchanged), md5
`273b59107428c7054a7c40755e3372ff` / `0016422a72f2c9c24dd79eb15d8355d2` in tree and room. Mutations: dropping
`put(run,"library_index",…)` → 755/757 naming both route cases; deleting `if lines.len() > LINE_CAP` → 755/757 with
the *previous-link* sentence as the left value, which is what proves the over-cap leg discriminates the cap arm from
the chain arm rather than merely observing some refusal. **Residual, and it is the next step, not a footnote:** the
run key has no product reader — the route writes it and three tests read it. Lane `g3c` is dispatched to make a
round consume it, so that history replaced or rolled back under a live run stops being silently accepted.

**`rrule1` landed as `56e1dbb`** (`tools/package.rs`, +341). The R rule finally has a native instrument behind it
instead of a table I re-ran by hand: `prepare()` runs `enforce_python_census` after its own walk and before
`screen_files`, before any artifact byte exists, and `publish` uses that path for real and `--dry-run` publishes
alike. `core/` and `platform/` must hold zero Python; `tools/` is allowlisted by exact relative name with the role
each file still serves; `verification/` is capped in aggregate at 73 files / 8,570 lines and can only shrink;
everything else refuses; `tooling`, `toolchain`, `.qwen`, `.playwright-mcp`, `output`, `__pycache__`, `.git` stay
out of the count. Census measured by the tool and re-checked with `find`/`grep`/`wc`: **core 0/0, platform 0/0,
tools 2/189, verification 73/8570, elsewhere 0/0**. Tools witness in the primary tree, exit 0: alignment 2,
readback 24, checkpoint 5, signature 5, apk 6, toolchain 7, gate 8, package 39 = **96 cases** (package.rs 29 → 39).

**Two instrument facts came out of that witness, and both cost me a false red.** A `git archive` room is **not a
valid tools witness**: `gate`'s own tests read `output/checks/full-units.json`, and `output/` is gitignored, so the
room has no such path. And under three concurrent builds, `checks::written_script_executes_on_the_first_attempt`
panicked on `Os { code: 26, kind: ExecutableFileBusy }` ("Text file busy") — twice, in two different trees — and
went green (`gate 8`, zero failures) on a quiet re-run. Both are harness facts, so `tools/` claims are witnessed in
the primary tree, alone.

**`g1s3` landed as `c396323`** (`core/self_build.rs`, the held xect/g1s2/g1s3 bundle). The successor view no longer
depends on `std::process::Child` caching a status it never promised to cache: `Tracked` carries `exit`/`signal`/
`exit_observed`, `observe_exit` stores the first observed status, `successor_states` calls it, `reap` records the
stored value. Before this a dead-but-unreaped child reported `running = true`. The missing-companion refusal is
narrowed to what the process can actually prove — journal has a row **and** `companion_tracked`, and production
stores exactly the companion path (`core/self_build.rs:1682`), so the predicate is reachable rather than decorative;
an unrelated journal with rows keeps the empty witness and really execs, proven by a sub-process probe with a proof
file. Room witness `git archive 966580b` plus that file: build exit 0, **763/763 plus 55**, checks exit 0, 12
warnings, md5 `2db510b4af767b95082a1092016bef35` in tree and room. Lane mutations red for the right reason: stored
exit removed → `reap_uses_the_exit_stored_by_the_view` (`Num("4")` vs `Num("9")`); predicate weakened to "journal has
rows" → `handoff_review_confirmed_execs_when_unrelated_journal_rows_have_no_tracked_successor`.

**Five audit seats ran against these three diffs; four of their P1s died on the code, and I only caught the
refutations by reading the files myself.** `journal_lines` is *not* unbounded — it refuses non-file/symlink,
`journal exceeds 1 MiB` and `journal exceeds 256 revisions`, with `NOFOLLOW` (`core/self_build.rs`, `JOURNAL_MAX`/
`JOURNAL_LINES`). `parse_library` checks the line count **before** the blank, record-size, parse and `linked_library`
legs, so an over-cap library cannot be misattributed to a broken chain. `companion_tracked` does not compare the
primary journal with the companion, because production tracks the companion. The real-tree census test *is* the
line-ceiling accept case: the tree sits at exactly 8,570 verification lines and `enforce_python_census(...).unwrap()`
passes there. What the seats genuinely earned: the publisher has **no publish fixture at exactly 8,570 lines** (the
accept side is proved only through `enforce_python_census`), its census reads each Python file whole with no size
bound, `verification/` refusals name a ceiling and not the file that grew, `core/self_build.rs:2140` quietly changed
an older fixture from "companion missing" to "companion present and empty" (the behaviour is covered by the dedicated
vanished-companion test, but the fixture no longer exercises that case), and a signal-killed successor renders
`exit: null` in the view while the durable row carries the signal — no view field distinguishes it. Lane `g1s4` is
dispatched on GAP1 stage 3: measure truncation, wholesale replacement, deletion and rollback of a successor journal
today, then give the chain a durable high-water that survives `exec`.

**Lanes in flight at 11:2Z:** `saz1` (`core/har_evidence.rs` +203, `core/observation_records.rs`, `core/archive.rs`
— the `.saz` container GAP4 still cannot read), `g3c` (`core/harness.rs`, `core/rrsi.rs` — make the run's
`library_index` a consumed product input), `g1s4` (`core/self_build.rs` — successor-journal high-water). Nothing of
theirs is committed. The connected record is still the pre-`436b281` one (71 groups / 4,215 checks / 67 pass / 4
unavailable); the sweep needs the primary tree, a clean tree and one quiet run, so it is queued behind these lanes.

## The successor chain gets attacked before its anchor exists, and the two runners' inventories turn out to be exactly equal — 2026-11-13T11:2Z–12:1Z (program clock)

Lane `g1s4` was still writing when its turn came up, so I attacked the **committed** chain (`c396323`) instead of waiting: `core/self_build.rs` read out of git (`git show c396323:core/self_build.rs`), not the lane's working copy. Every citation below is a line number in those committed bytes.

**Six accepted attacks — the yardstick `g1s4` is measured against.** None is refused today:

1. **Truncate** the companion to an earlier valid prefix. The tip is derived from the file — `let tip = rows.last().map(|row| text(row, "revision")).unwrap_or_default();` (`:999`) — so a shorter retained prefix reads green.
2. **Replace** the companion with a different internally valid chain. Every check is local to one row and its immediate predecessor (`durable_successor_rows`, `:1446`); no expected tip is supplied at the call site (`:1035`).
3. **Delete** the companion in a fresh process while the main journal still has rows. The only refusal is `if journal_has_rows && companion_tracked(&companion)` (`:993`), and `companion_tracked` walks the in-process table (`:1002`); after a restart there is no entry, so deletion reads as an empty witness.
4. **Roll back and re-append** a different valid suffix. No monotonic anchor exists, so a truncated-then-reextended chain verifies.
5. **A syntactically valid but unrelated `parent`.** `handoff` checks only its shape — `digest_text(&text(value, "parent"))?;` (`:1031`) — and never compares it with the tip it reads at `:1035`. Confirmed independently: across `core/`, `successor_tip` appears **only** inside `self_build.rs` (`:1035`, `:1041`, and tests) — no product reader anywhere else. That is the same defect shape as GAP3's `library_index`: a field published into a receipt and never acted upon.
6. **Empty-witness conflation.** `(0, "")` is returned for an empty `journal` field, for a missing untracked companion, and for a companion that exists but is zero-length, so the receipt cannot distinguish "no successor" from "successor witness gone".

**Two more seat findings refuted against the code.** (a) "Metadata/content substitution race between `symlink_metadata` and `open`": `journal_lines` opens with `NOFOLLOW` and hands that descriptor to `read_fd`, which `fstat`s it, reads `max + 1` and refuses `"file changed while reading"` when the byte count differs — a swap fails closed instead of adopting foreign bytes. (b) "The comment conflicts with the implementation": `The in-process table is not consulted.` (`:1445`) is the doc comment of `durable_successor_rows` (`:1446`), which genuinely never consults the table; the process-local refusal lives in a different function, `successor_witness`. (`companion_tracked` exact-path equality and `journal_lines` bounds were already refuted in the previous block and hold here: `locate` rebuilds the path component-by-component and refuses a symlink at every component in both modes, so the witness path `locate(root, &journal_rel, false)` cannot diverge from the tracked `Some(successor_journal_path(&locate(root, &journal_rel, true)?))` stored at `:1682`.)

**The two runner inventories are equal, and the publisher's Python ceiling has zero slack.** Measured on this tree: `tools/checks.py`'s group list is 71 names and `tools/harness.rs`'s `GROUPS` is 71 names, order-identical, symmetric difference empty — the interlock at `checks.py:50-51` actually passes rather than merely existing. The `verification/` census is 73 `.py` files = 71 groups + the one declared helper (`transport`) + the one declared optional (`live_web`), with zero unwired files and zero declared-but-missing files. `tools/package.rs` pins `VERIFICATION_PY_FILE_CEILING = 73`. Those two numbers are now identical, so **adding a connected verification group refuses `publish` until a Python file retires**: a new group is not free, and the retirement order matters (lane `gatesel` retires `checks.py --units` first).

## Two lanes land, the warning baseline goes to zero, and the saved-capture surface turns out to have no product input — 2026-11-13T12:1Z–13:2Z (program clock; `968b35c`, `925fed7`)

**`warnz1` — all twelve pinned warnings cleared at the cause (`968b35c`).** Five `AtomicU64::fetch_update` → `try_update` swaps (`core/approval_rules.rs`, `core/harness_rules.rs`, `core/inquiry.rs` ×2, `core/page_program.rs`), two needless `mut` removed (`core/state_crypto.rs`), and four dead items deleted: `elf_code::Context.symbols`, `resource_plan::Resource.id`, `resource_plan::InlineImage{node,target_span}`, `resource_url::Url::{scheme,host,port}`. I read the whole 194-line diff myself. I also proved the swap is behaviour-preserving rather than trusting the lane's probe, with a standalone probe on the sanctioned toolchain: `try_update` gives `Ok(7)` on success from 7, `Err(18446744073709551615)` at `u64::MAX`, and `Err(3)` when the closure returns `None` — the previous value on both sides, exactly `fetch_update`. Room witness (HEAD + those 9 files): exit 0, `Rust cases passed: 764/764; isolated application cases: 55`, and **`grep -c '^warning:'` on the build log is 0** — the 12-warning baseline is gone, so `core_warnings_pinned_12` is retired.

**My own witness caught a load-dependent red before I charged it.** The first run printed `763/764` with `self_build::checks::tracked_carried_identity_is_recorded_and_handed_to_the_successor` FAILED — a case in a file that lane never touched. Pure HEAD was green (`763/763`, 13 warning lines), a quiet re-run in the same room was `764/764` with 0 failures, and the case run directly five times was 5/5 green in 0.09 s. So it is a load-dependent flake in a case that execs a successor process, recorded as an instrument fact, not a red charged to the lane.

**`saz1` — SAZ archives now read through the existing unpacker (`925fed7`).** One line makes `archive::unpack` `pub(crate)`; `core/har_evidence.rs` gains the `PK` sniff, `session_kind`, request/response parsing and a 22-sentence refusal map; `core/observation_records.rs` gains 15 route-level tests that drive an archive through `query` rather than calling the parser. My independent room witness: exit 0, `778/778` plus 55, warnings unchanged; combined with `warnz1` the tree is **779/779 plus 55 with zero `warning:` lines**.

**The seat's strongest finding died; a bigger one that no seat found replaced it.** The `saz1` seat charged a P2 that "member byte and count caps are checked only after the complete ZIP is unpacked and buffered". Refuted: `core/archive.rs:866-871` bounds the expanded member to 1 MiB and the *total* to `"Project exceeds 4 MiB data limit"`, with the count capped at 500 (`bounded_size(count,500)`, `:791`) — the capture caps are refinements over an already-bounded buffer. What the seat never asked is **whether anything in the product can hand those bytes to the parser**. They cannot: the only consumer is `core/observation_records.rs:115`, which parses a retained record's `report` **string**, filled at `:56` from the tool's textual output passed in at `core/application.rs:3166`/`:3169`; and the connected witness never used a real route either — `verification/pe_declarations.py:56-60` does `row['report']=report` by hand. Since `report` is a JSON string, a real `.saz` (binary ZIP) can never reach it, and no tool's text output is ever a capture document. **GAP4 stays open** and lane `gap4route1` is dispatched to build the missing file route, with a required non-UTF-8 archive in the test.

**A regression my own committed lane introduced, caught from a seat's hedged P1.** The `warnz1` seat said the appended provider stop reason "changes observable output" but could not tell whether any consumer matched exactly. It does: `core/application.rs:727` is `if text(inputs,"purpose")!="model"||error!=super::provider::EMPTY_REPLY{return Ok(None);}` — exact equality against `core/provider.rs:3 pub const EMPTY_REPLY:&str="Server returned no text response"`, reached from `:3277-3279` for `network_result`/`failure`. The `content` protocol still emits the bare constant (`core/stream.rs:660`), but the `messages`/`chat`/`local` protocols now emit `"Server returned no text response: <reason>"`, so `recover_web_answer` silently skips its recovery arm exactly when the provider said why it stopped. Lane `wzfix1` is dispatched on `core/application.rs` to keep the reason while making the consumer accept the prefixed form, with a test that is red before the fix.

**Lanes in flight at 13:2Z:** `gatesel` (`tools/gate.rs`, native `--units` parity), `g1s4` (finished, unadjudicated: `core/self_build.rs` successor high-water), plus `wzfix1` and `gap4route1` just dispatched. `g3c` (`core/harness.rs`, `core/rrsi.rs`) and `toolc2` (`tools/package.rs`) are finished and still unadjudicated. The connected record is still the pre-`436b281` one (71 groups / 4,215 checks / 67 pass / 4 unavailable).

## The stored library index gains its consumer, the publisher census closes its residuals, and my own regression is repaired — 2026-11-13T13:2Z–14:4Z (program clock; `9a8573d`, `d5c8f9e`, `451c969`)

**`g3c` — GAP3's dead field is now read by the product (`9a8573d`).** The `library_index` rows that `resume_run` stores are compared, in `historical_experience` (`core/harness.rs:221`), against the library the propose/compose prompt is about to load; a row whose `key`/`revision`/`run_digest` is not the loaded row at that position, or a library that gained or lost a row, yields `stored library index does not match the library on disk; history was kept and no line was dropped` in place of the section. My room witness: `787/787 + 55`, exit 0, zero warnings, room digests equal to the lane's (`b38777aa…`, `5f68eec0…`).

Two seat findings needed my own census to settle. The seat asked whether a second production route still renders history unchecked: `render_historical` has exactly two callers, `historical_section_checked` and `resume_run`'s own prompt (`core/rrsi.rs:1437`) — and `resume_run` is where the index is *created*, so an unchecked render there is the definition of "first establishment", not a hole. The seat charged the positional `zip` comparison as a P1; it is the opposite of weaker — an append, an insert at position 0, and an equal-length reorder are all refused, so the guard is stricter than identity comparison. The arm that does remain is honest: `let Arr(rows) = stored else { return false; };` means a run whose stored `library_index` key was deleted skips the guard, and `resume_durable_ledger` is called only on the `refine` route (`core/harness.rs:451`), so `delegate`/`compact` rounds render history unchecked by design.

**`toolc2` — the publisher census residuals close (`d5c8f9e`).** A planted Python file is now refused from `metadata().len()` before its body is allocated (`PYTHON_CENSUS_BYTE_CAP = 1 MiB`, `take(cap + 1)`), a ceiling refusal names the largest eight verification files, and the accept side is finally proved by a **real publish at exactly 8,570 lines**. My tools witness in a room with `output/checks` copied in: exit 0, `alignment 2, readback 24, checkpoint 5, signature 5, apk 6, toolchain 7, gate 8, package 41` = 98, no `package` warnings. That run also retires an instrument belief of mine: a `git archive` room **is** a valid tools witness once `output/checks` is present, which is how I isolated this lane from `gatesel`'s in-flight `tools/gate.rs`.

The seat's P1 here was "symlink following can admit external Python into the census", charged on `fs::metadata`/`File::open`. Refuted as to outcome by ordering, which the seat could not see: `enforce_python_census(root, &files)` runs at `tools/package.rs:2765` and `screen_files` at `:2766`, and `screen_relative` returns `Err(refuse("symlink {relative}"))` at `:2075` — the symlink is refused, it is merely read first. What survives is a P2 of attribution: because the census runs first, a symlinked verification file that trips the ceiling reports a ceiling sentence instead of naming the symlink; and a file name containing a newline can add an attacker-controlled line to that sentence.

**`wzfix1` — my own regression repaired (`451c969`).** `recover_web_answer` now accepts the bare constant or the constant followed by `": "`, so a provider stop reason riding on the refusal no longer skips the web-answer recovery arm. Two tests drive the arm directly; the lane's mutation leg shows the red-at-base line `left: Ok(None)` / `right: Err("Search evidence is still in this conversation…")`. My room witness: `789/789` with **`isolated application cases: 57`** — the application-only count moves from 55, so the long-standing 55 pin is superseded. Residual: no test carries the suffixed string end to end from `Transfer::finish` through `network_result` into the consumer, so an existing connected group should grow that leg (a new group is blocked by the 73-file Python ceiling).

**Instrument discipline recorded this tick:** I again committed ahead of a seat (`g3c`), and once again the seat then produced a finding I had to settle myself. The order that works is witness → seat → commit, and `gatesel` (879 lines in `tools/gate.rs`, gate tests 8 → 17, byte-identical `selected-units.json` claimed against the Python document) is being adjudicated in that order rather than the reverse.

## The selected-unit route reaches byte parity everywhere except one code point, and two of my own instrument beliefs die doing it — 2026-11-13T15:0Z–15:5Z (program clock; `gatesel` held, not landed)

**Two instrument failures of mine, both silent.** My first `gatesel` differential produced `sha256sum` equality between `/tmp/gsw-py.json` and the room's `selected-units.json` — and proved nothing: `mkdir: cannot create directory '/tmp/gsw/bin': File exists` (the units binary was *named* `bin` in the directory I wanted the tools in) killed the tools build, so `/tmp/gsw/bin/gate: Not a directory` means the native leg never ran and I compared the Python document with its own copy. My corrected run then measured the **wrong tree**: `build-units.sh` and `build-tools.sh` derive `project_dir` from `dirname "$0"/..`, so invoking `/home/shawn/RLLM/tools/build-units.sh` from inside a room compiles the **primary** tree. The tell was 7 `function … is never used` warnings pointing at `core/file_evidence.rs` — that is lane `gap4route1`'s in-flight state, not HEAD; the room-local invocation (`$room/tools/build-units.sh`) rebuilt at `rc=0` with **`warnings=0`**, re-confirming the zero-warning baseline independently.

**The differential, done properly.** Room = `git archive HEAD` core + worktree `tools/`, `output/checks` copied in, three Python legs and two native legs against one units binary (`fec2e008b576f89e`, built warning-free): every leg `rc=0`, every leg printed `Selected cases: 292`, the 47 filter rows and the printed `<filter> <code>` stream are identical (`diff` = 0 lines), and the raw documents differ only inside the harness's own `finished in <s>` field. Normalising that field collapses **all five** documents to one digest `4930edf8c32b1750ebbe899c0ba9ad86d3eba317741d4c2e73db0ea171b7a6b7`. The lane had already reported the same shape (byte-identical on a deterministic binary, `Selected cases: 188`); my run confirms it on the real 789-case binary.

**Measured asymmetries from probes I designed, including one that killed my own hypothesis.** Trailing comma `{"selected_rust_filters":["a::","a::"],}`: I had read `skip_object` as accepting it — **refuted by running it**; native refuses (`verification/RESULTS.json`, `rc=1`) exactly as `json.loads` does. Duplicate filters: preserved on both sides — 21 rows and `Selected cases: 21` each, so no silent dedup. BOM: refused by both. `NaN` in a skipped value: Python accepts and runs 20 rows, native refuses — stricter, and acceptable under the R-rule parity test, but it is a real reachability change if a float ever lands as `NaN`. Deep nesting: at depth 20 000 native parses where Python raises `RecursionError`; at 200 000 native dies with `fatal runtime error: stack overflow` (`rc=-6`) where Python raises a catchable error — a P3 robustness gap in a hand-rolled recursive reader, unreachable for a record the harness itself writes.

**The defect that stops this lane.** A child whose chosen `test result:` line carries DEL: native wrote the raw `0x7f` byte, Python wrote `\u007f`, and `cmp` failed at **byte 141**. A full-payload run (12 264 characters, one run per instrument) moved the first difference to byte 239 and left the two *decoded* summaries identical — so this is purely an escaping-representation divergence, and `json_string` (`tools/gate.rs:135`) is the only site. An exhaustive comparison of the rule over U+0000–U+017F plus the astral and separator cases gives **exactly one** divergent code point: `0x7f` (Python emits `"\u007f"`, the Rust rule emits the raw byte, because its guard is `c < 0x20 || c > 0x7f`). C0 controls, `\b \f \n \r \t`, `"`, `\`, `/`, U+00A0, U+2028/2029, C1 controls and astral surrogate-pair emission all match. The lane's claim is "matching `checks.py --units` **byte for byte**", so the claim is currently false and `tools/gate.rs` is **held uncommitted** rather than landed with a residual.

**Seat findings, all adjudicated against the code or a probe.** Seat 1's P1 "duplicates may be deduplicated" — refuted by the 21/21 probe. Its P1 "the binary path may not be resolved" — refuted by reading `resolve_binary` (`tools/gate.rs:330`), which `canonicalize()`es, the `Path.resolve()` equivalent. Its locale-decoding P1s are environmental (the sanctioned env runs UTF-8), and its print-vs-report-file ordering P2 is real but only observability: the row order and content of the printed stream are identical, and a mid-run refusal leaves Python with prior rows printed and native printing none. Its ETXTBSY P1 is native-only robustness — Python would crash where native retries and proceeds. Seat 3's claim that DEL diverges is the one that mattered, and measurement agreed with the seat, not with my own reading. Seat 3's "trailing comma accepted" reading of the pseudocode was refuted by the same probe that refuted mine.

## The successor mark stops being a quadratic journal, the saved capture gets a real file route, and one open residual turns out not to exist — 2026-11-13T15:5Z–16:4Z (program clock; `59d13d6`, `f22ded0`)

**`mark1` — the terminal self-build failure is closed (`59d13d6`).** `append_mark` used to seal one row per append, each row carrying the whole companion chain, so the mark grew quadratically and crossed the 1 MiB reader bound at append #174 (my simulation predicted 1,054,527 bytes; the lane's red line reads `successor mark exceeded its bound at append 174 (1054463 bytes)` — two independent measurements agreeing on the same append). It now seals **one anchor row** whose `revisions` are the whole companion and whose `parent` is empty, published by `rewrite_mark`: a `.tmp` file with `O_CREAT|O_EXCL|O_NOFOLLOW` mode 0600, `sync_all`, then `rename` over the mark, with the existing mark and its parent directory refused up front if either is a symlink. Soak test `successor_mark_stays_bounded_past_append_174` runs 200 appends asserting the bound after **every** append, then asserts one content-addressed row, `parent == ""`, `revisions` equal to the whole companion, `handoff` reporting `successor_rows 200`, and — after truncation, replacement and fork refusals — that the mark **bytes are unchanged** by a refusal. All seven refusal sentences survive verbatim.

The seat charged three P1s about losing the chained form. The most important is refuted by the reader's own semantics: the old chained mark was prefix-valid, so restoring an *older* chained prefix was already accepted — there was no rollback-of-evidence protection to lose. Its "silent healing" P1 (the writer no longer reads the previous mark) is refuted by ordering: `append_linked` calls `recorded_successor_revisions` (`core/self_build.rs:1586`) and `successor_chain_break` (`:1592`) **before** `append_mark` (`:1598`), and `revisions` is derived from the companion on disk (`durable_successor_rows`), never from the caller — so a corrupt or replaced chain refuses before it can be overwritten. What survives is one real P2 the seat found and I confirmed: **`rewrite_mark` never syncs the parent directory after the rename**. An append needs no directory sync; a rename changes the directory, so after a power loss the rename can be lost while the companion append survives, leaving an anchor one append stale — a weaker witness, not a false pass. Repair lane `markdir1` dispatched. The both-files-deleted residual is closed by the lane's plain statement, which I verified against the writers: nothing durable pins the chain once `journal.successors` and the mark are gone (the build-revision row stores the pid/argv/digests, not the revision list), and borrowing it would move the build-revision tail the witness is required to leave alone.

**`gap4route1` — GAP4 finally has a product input path (`f22ded0`).** Until now the saved-capture surface had no input: its only consumer parsed a retained JSON *report string*, which a binary `.saz` can never be. `investigation::inspect` now routes paths ending in lowercase `har`/`saz` to `file_evidence::saved_file_capture`, which reuses the `observe` guards (read permission, response-allowance floor, `Guard`, `Held::open`/`Held::scan`), refuses above `archive::UNPACK_BYTE_LIMIT` (now one shared constant instead of a duplicated literal) with `Refusal: saved capture file exceeds the reader limit.`, and hands the whole file to `har_evidence::file_capture`, which always returns a typed view with **named** refusals. `observation_records::query` gained one fallback branch that lifts the already-parsed view out of the inspect display via `section_capture`, whose `section_view` validator requires the exact six-key row shape before publishing anything. The fixture `.saz` carries DOS time `0x8120`, so the archive contains byte 0x81 and `std::str::from_utf8` fails on it — a non-UTF-8 archive parsed from disk, which is the thing the report-string route could not express.

My own probes of the seat's claims: its P1 that the whole `inspect` now fails on permission/budget is largely theoretical — `investigation::inspect` is handed a `Source` already opened under the *same* `access`, so those guards are redundant re-validations, and a permission/rules change mid-operation is exactly what `Guard::check` is for ("Current rules changed during the complete-file operation"). Its ZIP-expansion worry is answered by the pre-existing member caps (320 members, 64 KiB each) plus the 5 MiB input bound. What is real and stays open: `kept` buffers up to 5 MiB in a request path whose admission floor is only 96 KiB; the extension match is case-sensitive, so `capture.HAR` never reaches the parser; the `content: complete_digest` inspect arm still produces no capture rows (its dispatch lives in `core/application.rs`, which this lane was fenced out of); and whether `Held::binding` detects a same-length in-place rewrite between the two reads is **unverified**, so a mixed-version display cannot be ruled out. And the connected witness is still the stub the gap report forbids — `verification/pe_declarations.py` injects `row['report'] = report` — so lane `gap4conn1` is dispatched to drive a real file through the product inside that group, under the hard constraint that `verification/*.py` sits at exactly 8,570 lines with **zero** slack against `VERIFICATION_PY_LINE_CEILING`.

**A residual I had recorded is refuted, not fixed.** `gap3_index_guard_residuals` item (b) claimed `delegate`/`compact` rounds render history unchecked. Lane `g3route1` censused the call sites and found those phases never render the experience library at all, and my own grep agrees: `historical_experience` (`core/harness.rs:216`) has exactly one product call site (`core/harness.rs:662`, the guarded one), and `render_historical` has exactly two — `historical_section_checked` (`core/rrsi.rs:1380`) and `resume_run`'s own prompt (`core/rrsi.rs:1448`), which is where the index is created. The lane changed nothing, made no build (correctly: a build of a tree holding other lanes' uncommitted edits would not have been a witness of anything), and the remaining open half is item (a) — a stored `library_index` that is not an array silently passes the guard — which lane `g3arr1` is closing now.

**Witness.** One combined room for both landed lanes: `git archive HEAD` core plus exactly those six lane files, room-local `build-units.sh`, `units rc=0`, digest `da492eb419a3b448`, **`warnings=0`**, then `Rust cases passed: 792/792; isolated application cases: 57`, sweep `rc=0`, `failures 0`. That is HEAD's 789 plus exactly the one soak test (`59d13d6` → 790) and exactly the two capture-file tests (`f22ded0` → 792) — each lane's own room count matched the arithmetic before I ran it.

## The recursion yardstick finally gets scored line by line, one silent-pass guard closes, and two connected legs stop being stubs — 2026-11-13T16:4Z–17:5Z (program clock; `de6cd85`, `6c114ce`, `gap4conn1` witnessed and awaiting commit)

**Gap 2 scored feature by feature, not by impression (ledger `gap2_feature_census_2026_11_13`).** Ten of the thirteen features are in the product with file:line evidence: the multi-round harness loop (`open_next_round` `core/harness.rs:1028`), the eight named edit components (`core/rrsi.rs:29`), the annealed budget (`BUDGET_MAX`/`BUDGET_MIN` `:96`-`:98`, `annealed_budget` `:684`), evidence-aware edit history with the do-not-repeat refusal (`prior_rejection` `:726`, refusal `:716`, prompt line `:862`), the leakage critic before scoring (`improvement.rs:509`, `critic_against` `core/harness.rs:176`), the noise floor measured from repeated base rows (`improvement.rs:183`), structural pruning (`pruned_components` `:790`, `PRUNE_WINDOW` `:47`), the evolve/held-out/OOD split (`core/harness.rs:708`-`:723`, `SPLIT_KEYS` `:68`-`:83`), incumbent-kept when nothing is admissible (`:683`/`:728`/`:732`), and git-worktree candidates on a fast-forward evolve branch (`WORKTREE_MAX` `:87`, `EVOLVE_BRANCH` `:88`). Three are genuinely open: **feature 5 stall exploration** — the untried list exists (`untried_components` `:828`, prompt line `:889`-`:893`, library field `:1035`) but nothing computes "progress over *w* rounds inside δ" and nothing reserves a slot, and the word *stall* survives only as a comment at `:35`; **feature 8 cost-aware acceptance** — `improvement.rs:133` has only the qualitative `lower cost` / `cost rose` arms, no `ΔC ≤ β0 + β1·ΔS` bound and no structural-novelty arm (zero β or novelty hits anywhere in `core/`); and **feature 13** — JSONL history, `resume_run` `:1430`-`:1470` and the plain-file candidate body (`HARNESS_FILE` `:91`, committed `:1765`) exist, but there is **no STOP file** in the round path and **no re-judging** (zero `rejudge|re_judge|re-judg` hits in `core/`). One honest caveat against my own earlier phrasing: the candidate pool is not concurrent — the code's own comment at `:84`-`:85` says the slots are judged in order and there is no thread schedule in the ledger. Four briefs are staged for the missing halves (`gap2stall1`, `gap2cost1`, `gap2stop1`, `gap2prompt1`).

**`g3arr1` — a stored `library_index` that is not a list used to be *agreement* (`de6cd85`).** `index_disagrees` began `let Arr(rows) = stored else { return false; }`, so a string, object or number in that key silently passed the guard and the propose prompt rendered history the run had never resumed. `historical_section_checked` now matches `Null` (render), a disagreeing `Arr` (`LIBRARY_INDEX_MISMATCH`), an agreeing `Arr`, and everything else with a new named sentence: `stored library index is not a list; history was kept and no line was dropped`. Room witness (`git archive HEAD` plus only that file, room-local `build-units.sh`, disjoint units path): `units rc=0`, **`warnings=0`**, binary digest `ffdc6c77748273e5`, then **`Rust cases passed: 794/794; isolated application cases: 57`**, `test result: FAILED` count 0 — 792 pinned at `f22ded0` plus exactly the two new cases. The seat's reachability finding is confirmed by my own census: `index_disagrees` has exactly one caller, whose guard passes it only `Arr`, so the arm it changed is decorative and the new `match` is what closes the hole. Two seat items marked UNVERIFIABLE I settled by reading: a run cannot make its own install-time index stale, because the only library append (`record_settled_library` `core/harness.rs:270`) is called solely from `finish_refinement` (`:1225`) which ends the run in `review`, while history renders only for phase `propose|compose` (`:657`-`:662`); and a corrupt value is replaced, because a fresh refine/resume re-installs `library_index` from the resume receipt at `:267`.

**`websuf1` — the web-answer recovery path is now proven end-to-end (`6c114ce`).** The `commands` group drives a real streamed chat turn: the SSE proposal dispatches `web_search` with no review dialog, the search evidence returns into a streamed follow-up prompt, an empty completion with `finish_reason: content_filter` yields exactly `Server returned no text response: content_filter` from the product parser, and the failure callback must produce `Search evidence saved. Asking the same model to answer from those results…` with `web_answer_only` true and the evidence still in the wire — then a second empty completion must end at the fixed refusal sentence in notice, `ui.status` and `ui.visible_status` with no effect. My own witness, two rooms on **one** product binary (`c13c331871c8ab7c`, built from HEAD in the room): HEAD+leg `rc=0` with **86/86 checks** including the seven new names; HEAD alone `rc=0` with **79/79** and none of them. `verification/` stayed at **73 files / 8,570 lines** — exactly the pinned ceiling, so the leg was paid for by reflowing existing lines, and `commands.py` is still 72 lines. The lane's neuter (comparison replaced by exact equality against `EMPTY_REPLY`) named the failing check, and I reproduced the neuter in the room. Coverage residuals I accept and record rather than fix: the second empty completion is fed through `stream_finish` although the recovery effect is not streamed, so the real host's bare-error path (`Server returned no text response`, no suffix) is not exercised; and because the eligibility check at `core/application.rs:737` compares a recomputed profile digest plus owner, a mid-chat model change (a supported feature) makes recovery silently ineligible — fail-closed, but the user just sees the bare error.

**`gap4conn1` — the saved-capture route finally has a file on disk inside the connected record (witness green, commit pending).** `verification/pe_declarations.py` writes `traffic.har` (one entry, `GET https://example.test/from-file`), a stored-ZIP `traffic.saz` whose local and central headers carry DOS time `0x8120` (so byte `0x81` makes the file non-UTF-8 — the thing a report-string fixture can never express), and `notes.har` containing plain text. Each goes through the real approve-then-execute inspect path against the scratch workspace, and the assertions require the typed rows, the retained record's `saved_capture` equal to the inspect section, the retained `report` equal to the inspect display (not a string assigned by the test), and the file still holding the bytes just written. My witness used two HEAD rooms sharing one product binary: **37/37** with the leg, **34/34** without, the three new names exactly as reported, nothing removed, and the group's `scope` line extended to name the new coverage. The line ceiling held at 8,570 with `pe_declarations.py` still 132 lines. What still is *not* proven: the six older saved-capture checks keep injecting `row['report'] = report` on a folder record, and truncated captures, a malformed sibling member, the 320-member and 64 KiB member caps, the 5 MiB file cap and the uppercase `.HAR` case stay on that string path only.


## Two product routes get proven by taking the product apart, and a renamed mark stops being a directory-entry gamble — 2026-11-13T17:5Z–18:5Z (program clock; host clock 2026-11-12T18:0Z–18:55Z; `285e8f3`, `7b7125d`, `da5b49e`, `43cec7d`)

**`markdir1` — the anchor mark's rename was fsynced nowhere (`285e8f3`).** `rewrite_mark` wrote a temp file, `sync_all`'d the *file*, and renamed it, never touching the parent directory; a rename is a directory entry, so a crash could put an older anchor back in place under a companion journal that already carried the new row. It now opens the parent with `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` and syncs it after the rename. The seat's two *real* findings are accepted and recorded rather than argued away: (a) the result is deliberately discarded, so the durability of that rename is **attempted, not confirmed** — with `let _ =`, even a real `EIO` on the directory fd is invisible, which also means *no test can observe the success path*; (b) propagating the error would instead fail `append_linked` after the companion row and the mark already agree, and a caller that then deleted the mark would make every later handoff refuse an intact chain. The reverse crash window the seat was asked about (mark durable naming *N+1*, companion holding *N*) is a **correct refusal**, not a false one — that pair is what `successor journal is missing after a recorded chain` exists for; the seat could not name it from the paste, and my census of `append_linked` confirms it. The lane's reachability proof (panic inside `sync_mark_directory`, stack `rewrite_mark → append_mark → append_linked → append_spawn_row`) plus the `Ok(())` mutant (one test then fails on `unwrap_err`) is the strongest *in-process* evidence available while the result is dropped. Room witness: `units rc=0`, `warnings=0`, digest `8650f6bc050750bf`, **`Rust cases passed: 796/796; isolated application cases: 57`**, `FAILED` rows 0, `#[test]` 58 vs HEAD 56.

**`gap4conn1` — the seat's blocking provenance charge was answered by disassembling the product, not by more assertions (`7b7125d`).** The seat read the three new file legs and ruled *blocking*: nothing in them proves the product opened the file rather than a report string. So I ran two targeted neuters against the same group, each a rebuild of the product in the room. Neuter one: `Some("har")|Some("saz")` → `harx|sazx` (product `182461d4157a53ef`) — group `rc=1`, and the assertion payload shows the HAR file rendered as an ordinary **static inspection**, i.e. no capture section at all. Neuter two, narrower: `Some("har")|Some("sazx")` (product `a27e17f23895c6ee`) — the HAR legs still pass and the **SAZ leg** is the one that reddens, with the archive rendered as an ASCII/hex dump. Restoring HEAD bytes returns `rc=0` with **37 checks** and the product digest back to `c13c331871c8ab7c`. That is per-leg attribution: a run that aborts at the first red cannot convict the legs behind it, so the second neuter was mandatory, and it is also why `check()` names are never enough on their own. The plain-text `notes.har` refusal leg is *not* neuter-proven — it sits behind the saz leg — and its claim is instead pinned **inside one run**: same extension, different bytes, rows versus `Refusal: file is not a saved capture.`, so a name-only refusal cannot satisfy both legs. Two other seat findings fall to census: `cap.get('caps')==[]` is not vacuous (`core/har_evidence.rs:453` writes the field, `:493` requires it in the row-shape validator), and the record-identity hazard is closed by `file_inspect` taking the last retained item **and** asserting `rec['args']['path']==path`. The seat's boundary list (member cap, member size cap, 5 MiB file cap) is dispatched as `gap4cap1`.

**`gap4edge1` — recorder files in either ASCII case reach the capture route (`da5b49e`).** The lane first reported "keep the refusal, no code change" and then landed the narrow fold: only the capture matcher becomes `eq_ignore_ascii_case`, so `.HAR`/`.Har`/`.SAZ` enter the same arm while the source, page and stylesheet arms stay exact. The seat suspected a case-dependent identity elsewhere; census says otherwise — the retained record keys the view under the path *as written*, and no other dispatcher routes by capture extension, so a shared helper is not required. Edge behaviour I checked rather than assumed: `.har`, `.HAR` and `TRAFFIC.HaR` are accepted; `traffic.har ` (trailing space), `traffic.har.gz` and a non-UTF-8 extension are not, because `Path::extension()+to_str()` is still the gate. The lane's own test guards the widening: `notes.TXT` must contain neither the capture marker nor the refusal sentence and must equal `notes.txt` after the name substitution. Room witness (one binary holding both this and `markdir1`'s file at the then-HEAD): **`Rust cases passed: 797/797; isolated application cases: 57`**, `warnings=0`, `FAILED` rows 0, digest `6c28befe421ead6a`, `#[test]` 21 vs HEAD 20.

**`gatepy1` — the Python-debt census now names what the native twin already does (witnessed, commit pending).** `checks.py`'s role string moves from three bare clauses to `full-suite unit gate (--all-units, native twin gate <units>)`, `selected-unit runner (--units, native twin gate --units)`, and `connected-group runner (no-flag sweep, Python-only)`, with the two census assertions repinned and a new runtime test that reads the const and refuses a Python-only label on either native-equivalent clause. The seat called the `<units>` spelling ambiguous; it is not — `tools/gate.rs:2`-`:4` states the contract: `gate <unit-binary>` *is* the `--all-units` role and `gate --units <unit-binary>` *is* the `--units` role. The seat's other real worry (could this role text be pinned somewhere else?) is a refutation by census: grepping `full-suite unit gate|selected-unit runner|connected-group runner` outside `tools/package.rs` returns **zero** files, so the two repinned assertions inside that file are the complete set.


## Two acceptance rules get a price, a stall rule gets a reserved slot, and three capture limits are pinned at their exact bytes — 2026-11-12T18:55Z–19:20Z (host clock; `75c630e`, `12161cd`)

**`gap1sig1` — the handoff view can now say *how* a successor died (`75c630e`).** A child killed by a signal used to render `running:false, exit:null`, byte-identical to a death the process never observed. `successor_states()` now emits `signal` beside `exit`, non-null only once `exit_observed` is set, so a normal exit, a signal death and an unobserved death are three distinct views; the tests drive a real `/bin/sleep 30` through `/bin/kill -KILL` and `wait_dead`, pin the rendered text, pin the durable reap row's key list `[event,exit,job,parent,pid,revision,schema,signal]` and its `exit`-before-`signal` ordering, and pin that reading the handoff appends **no** journal row and writes **no** mark. Room witness (`git archive ffbc4a7` + only this file): **`Rust cases passed: 799/799; isolated application cases: 57`**, `warnings=0`, digest `a1b697ad40fe4793`, `#[test]` 60 vs 58. The seat's three UNVERIFIABLE charges settled from code: `remember_exit` (`core/self_build.rs:1701`) writes `exit`, `signal` and `exit_observed` in one step and clears `child` only afterwards (`:1707`), and the durable row already carried `signal` via `witnessed_code` (`:1644`) with its key list pinned at `:4432` — so the new member is additive and in-process only. One residual kept: the view omits the `code >= 0` guard the durable path applies.

**Gap 2 feature 8 — an edit's cost now has a price, and a shape gets used once (`12161cd`).** `COST_BETA0 = 0`, `COST_BETA1 = 64`, enforced as `ΔC ≤ β0 + β1·ΔS`: one extra passed check buys at most 64 bytes of instruction text, and a score that did not rise buys none, which is what the pre-existing `cost rose` arm already said, now written as a bound. A positive gain whose cost rise the score cannot pay for is refused with reason `cost bound` and the sentence *"refused because the cost bound was exceeded for the gain N"*. Novelty is the triple the ledger already stores — `component`, `hypothesis`, `diff_digest`: inside a measured band (`floor > 0`) a flat, check-preserving score is admitted once for a shape absent from the run's rows (`structural novelty`) and refused the second time (`duplicate shape`). The reachability worry is settled by construction: `judged_acceptance` builds the triple exactly the way `note_refinement_edit` writes the row (`core/harness.rs:106`-`:114` — `declared_edit`, `improvement::stored_hypothesis`, `rrsi::diff_digest(prior, edit_body)`), so `shape_recorded` can match real product rows rather than only fixtures. `beta0`, `beta1` and `accept_reason` joined `SPLIT_KEYS` as optional fields, so a legacy row still seals.

**Gap 2's stall rule reserves a slot, and only a live δ may enforce it.** `progress_stalled` requires `STALL_WINDOW = 3` **scored** rounds whose best gain across every component stays inside δ, and `reserved_slot_refusal` then refuses a component already tried while an untried one still holds a slot — at both the critic site (`core/harness.rs:161`-`:164`) and the acceptance policy (`:964`-`:965`), each computing δ live through `improvement::noise_floor`. `δ = 0` (no estimated floor) leaves a run unstalled, so an unestimated run never reserves slots on every flat ledger. The two new prompt lines (*"Progress stalled inside the noise band for 3 rounds."*, *"Reserved slots for components not yet tried: …"*) are driven by the **recorded** floor, which is display, not enforcement; the tests pin the band edges (`gain == δ` inside, `δ+1` clears), that the round's best gain governs, that an unscored round is not a scored round, that only the last window counts, and that a quiet ledger renders byte-identically.

**Mutation legs, not narrative, for both new arms.** A second room (`/tmp/g2m`, HEAD + the three lane files) neutered one arm at a time. `Some(delta) => false && delta > cost_limit(gain)` reddens `cost_bound_refuses_a_rise_the_score_gain_does_not_pay_for` at `core/improvement.rs:928` — `assertion failed: !boolean(&over, "admit")` — and `if false && shape_recorded(seen, &shape)` reddens `noise_band_admits_a_new_shape_and_refuses_the_duplicate` at `core/improvement.rs:969` — `assertion failed: !boolean(&second, "admit")`. Both mutants compile; neither red is a build error. The restore rebuilt the units binary **byte-identically** (`baa7b710265ecdaa`, the same digest the pre-mutation witness recorded) with `warnings=0`, and the two named tests went green again. The lane's own witness (`/tmp/g2w`, `git archive ffbc4a7` + only those three files) is what the commit matches: **`Rust cases passed: 804/804; isolated application cases: 57`**, `warnings=0`, `#[test]` rrsi 42 vs 38, improvement 9 vs 6. Because `/tmp/s1w` isolated `core/self_build.rs` alone, each of the two commits matches an independently witnessed tree state rather than a merged blur.

**Two seat charges that did not survive contact with the code, and one that did.** The cost seat read `validate_policy`'s new arms as letting negative or fractional betas through: refuted — `number_field` (`core/rrsi.rs:133`-`:138`) parses `u64`, so `-1` and `1.5` are rejected as `invalid beta0`, and the `1_000_000` cap copies the existing `noise_floor` / `edit_budget` / `passed` convention (`:269`, `:320`, `:326`). Its search for a path where a novel shape bypasses a refusal it must not bypass found none: the arm is gated on `reason == "score"`. What is real is the quieter charge — `beta0`, `beta1` and `accept_reason` are written and sealed and **nothing in the product reads them back**, so enforcement still uses the constants; the re-judging lane (`gap2stop1`) is now briefed to be that reader, including a test that a row whose sealed `beta1` differs from the constant re-derives against the row. Also real and minor: `accept_reason` is bounded text with no vocabulary check, and the retain-probe path writes `"rejected revision"` (`core/harness.rs:196`), a value outside the seven acceptance reasons.

**`gap4cap1` — three saved-capture limits pinned at their exact bytes (witness green, commit pending).** Six legs in `verification/pe_declarations.py` straddle `MEMBER_CAP` 320/321, `MEMBER_BYTE_CAP` 65536/65537 and the reader limit 5242880/5242881, and the group moves **37 → 43** with `caps == []` on every side because all three are refusals, not caps. The Python ceiling held with **zero slack** — I measured `verification/` at exactly **73 `.py` files and 8570 lines**, and `pe_declarations.py` at 132 lines (34 added, 34 removed). The price is visible in the diff and worth stating: the module's opening docstring was deleted to buy a line, and `markers` / `drive` / `seal` / `verdicts` were reflowed to one-liners. I compared `markers` line by line and it is behaviour-preserving (blank-line skip, unknown-line reject, double-provenance reject, malformed JSON still raises). The lane neutered only the member-count leg and labelled the other two UNPROVEN, which is why I ran them myself in `/tmp/g4b` against the HEAD product binary `8c6aaa9a7bf2907f`: `MEMBER_BYTE_CAP 64 * 1024 → -1` aborts the group with `AssertionError: a response section of exactly 65536 bytes returns that body size and the member byte cap stays silent`, `UNPACK_BYTE_LIMIT 5 MiB → -1` aborts with `AssertionError: a saved capture of exactly 5242880 bytes returns the session row and is inside the reader limit`, and each restore returned the group to **43/43** with the binary byte-identical to the baseline (`har_evidence` back to `5309dc9e435dcc83`, `archive` back to `ade875af2651ffc2`). One correction to the lane's own account: it says the tighter capture caps "refuse first". They do not — `har_evidence.rs:399` calls `archive::unpack` **first**, and that reader's 500-member (`archive.rs:793`), 1 MiB-per-member (`:866`) and 4 MiB-total (`:870`) gates are simply looser, which is exactly what `har_evidence.rs:9` documents as the reason the capture caps exist. The fixtures assert `sum(len(d)) ≤ 4 MiB` precisely so that gate stays silent and the file limit is the only thing being tested.

**Gap 5 measured one step further, and its publication bar is externally gated.** In a room from `03d4d10`, `bash package.sh` with `CLIENT_UNSIGNED=1 CLIENT_VERSION_CODE=1795000001` completes (`rc=0`): `Alignment payload validation: 25 exact members`, `RLLM-unsigned.apk` 7 538 583 bytes `sha256 1ad646a0f40953ef`, `manifest versionCode=1795000001`. The native `apk` tool then refuses that artifact — `ValueError: APK signature verification failed: RLLM-unsigned.apk` — which is correct behaviour and the reason the publication chain cannot close: there is no `rllm.p12`, `*.jks` or `rllm.password` anywhere under `/home/shawn`, `/mnt` or `/media`, the newest stamp directory ends at `RLLM-unsigned.apk`, `apksigner verify` on it reports `DOES NOT VERIFY / ERROR: Missing META-INF/MANIFEST.MF`, and the published companion APK that `apk_alignment` and `turn_background` read is absent. Minting a keystore is forbidden by the rule that the retained identity `ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5` must be reused, so that leg waits on the owner's protected signing archive, not on code.

**Gap 3's remaining halves are now a census, not a guess.** The library half is **not** missing: `experience.jsonl` is written at `core/harness.rs:284` and read back at `:226` (`historical_section_checked` over the stored `library_index`) and `:256` (`load_experience`), so "later tasks never read the library" would be wrong. What measures as absent is `grep -rn "invariant|domain"` over `core/rrsi.rs core/harness.rs core/improvement.rs core/harness_rules.rs` → **0** hits (Gap 3 item 5: invariant-versus-domain scoping for every proposal) and `anonym` / `selector` over `core/*.rs` → **0** hits outside the CSS parser (item 7: choosing among anonymised deliverables when k > 1). `CANDIDATE_MAX = 3` with one worktree per slot is already the parallel-candidate pool, so item 2 is partially there and item 7 is the honest gap.

## The unit line gets a measured baseline, the JS surface is byte-decided, and the STOP loop and the native sweep twin are refused — 2026-11-12T14:05Z–15:00Z (host clock; `87c1638` → `b4b6ee9`)

### A baseline, not a chain of deltas

Every unit-line figure in this audit had been carried forward as a chain of per-commit `#[test]`
deltas. The chain is now anchored by a direct measurement: a room of `git archive HEAD` bytes and
nothing else (`tools/build-units.sh`, `python3 tools/checks.py --all-units`) printed
**`Rust cases passed: 806/806; isolated application cases: 57`**, build rc=0 with zero `warning:`
lines, units digest `616b17aab2452e66d1c640056e5f7979dd38abc599a29d3ad3b78de67c520824`, with
`core/harness.rs` 75 and `core/rrsi.rs` 42 `#[test]`. That corrects the 804 figure carried into
this window: two cases had landed in commits whose deltas were never re-summed. With the anchor in
place both lanes' self-reports became arithmetic identities instead of claims — 811 = 806 + 5
(`core/harness.rs` 75 → 80) and 816 = 806 + 10 (`core/js_evidence.rs` 5 → 14,
`core/file_evidence.rs` 6 → 7).

### Gap 4 item 3 committed: kind comes from bytes, not from a file name

`core/js_evidence.rs` (+665/−112), `core/file_evidence.rs` (+19), `core/investigation.rs` (+25/−1)
committed at `b4b6ee9`. The lane's claim was a room witness of 816/816; the coordinator's room was
independent (`git archive HEAD` plus exactly those three files, room shas equal to the worktree
shas) and printed the same line — **816/816 plus 57 isolated cases**, build rc=0, zero warnings,
units digest `2b8f55d98e9bebed6018648a69af5d5b4fdd8ad9ed4e4f180e7c4a9455f9e1ff`. Two audit seats
read the whole diff; three of their findings were refuted by the code:

- "the lexer sets module kind on any `(` token, so prose with `foo()` is a module" — the `(` arm
  lives inside `after_import` (`core/js_evidence.rs:851-860`) and `after_require` only fires after
  the word `require` plus `(`, which is the rule the file's own doc comment states ("a file is a
  JavaScript module only when its bytes contain an import, an export declaration, or a `require`
  call"), and the prose-with-`import` refusal is pinned by a test.
- "the 5 MiB refusal may come after the bytes are read" — the read loop in
  `core/investigation.rs:91-96` is itself bounded at 4 MiB+1, and `core/file_evidence.rs:87-89`
  refuses on `file_len` before any parsing.
- "an attacker-controlled ASAR header causes an arbitrarily large allocation" — `asar_directory`
  rejects `header_len > bytes.len() - 8` and cross-checks the pickle against it, and the project's
  JSON parser caps text at 8 MiB and nesting depth at 64 (`core/lib.rs:278-280`,
  `core/lib.rs:175-176`, pinned by `depth_limit` at `core/lib.rs:554`). The 128-entry cap bounds the
  listing, not the parse, and parse work stays proportional to the input.

What remains open on this item is test coverage, not behaviour: the new
`javascript native-addon cap` (64) has no dedicated test, and the lane's claim that "each parser
arm was shown failing with that arm neutered" is not witnessed — its transcript shows one batch red
run, not per-arm legs. Both are tracked as open items.

### Gap 2 item 13 refused before commit: a STOP file that the round cap walks past

The STOP-file/rejudge pair (`core/rrsi.rs` +65/−1, `core/harness.rs` +304/−0) was witnessed green in
a room holding only those two files — **811/811 plus 57**, zero warnings — and still was **not**
committed, because the audit found five real defects in it:

- `stop_requested` is consulted only at the top of `open_next_round`, and the round-cap branch
  follows it immediately, so a run with `refinement_rounds < 3` never consults the STOP file and
  the hand-off from the last allowed round into evaluation records no STOP row at all.
- the synthetic STOP row carries `component = "prompt"`, `component_counts`
  (`core/rrsi.rs:694`) counts every row of a run regardless of verdict or score, and `prompt` is in
  `COMPONENTS` (`core/rrsi.rs:25`), so a control row makes `prompt` look *tried*: it disappears
  from `untried_components` (`core/rrsi.rs:854`) and can drive `reserved_slot_refusal`
  (`core/rrsi.rs:911`) to refuse a genuine candidate citing a row that was never a trial. The gain
  path is clean for the same measured reason — `edit_gain` returns `None` for unscored rows, so
  `round_best_gains`, `component_rounds` and `pruned_components` already ignore it.
- the validator accepts any score-less refusal whose text equals the STOP sentence
  (`core/rrsi.rs:201-205`) with no structural pairing to the row that produced it.
- `rejudge` does not judge with a row's sealed betas: `costs_for_betas`
  (`core/harness.rs:1065-1084`) synthesises a cost pair so the *current* constants reproduce the
  recorded verdict, and `improvement::acceptance` takes no beta parameters
  (`core/improvement.rs:172-180`), so the reported reason always comes from current constants
  (`core/improvement.rs:210-212`). The row already stores `beta0`/`beta1`, so threading them is the
  honest fix.
- no mutation legs were run for any of these guards.

Two claims were refuted while adjudicating: the seat's "the sealed bound is never evaluated"
(`core/harness.rs:1072` computes `over_row = delta > sealed bound` and steers with it), and the
coordinator's own suspicion that the rejudge seen-set is wrong (the live path at
`core/harness.rs:962-964` reads `records` from the ledger *before* the candidate row is appended,
which is exactly `records[0..index]`). A repair lane was dispatched with these six findings; the
pair stays uncommitted.

### Gap 5: the native sweep twin is refused, and the Python publisher is revealed stale

The native `harness sweep` twin (`tools/harness.rs` +458/−39, `tools/package.rs` census wording) was
reaped, read in full, and **not committed**. Nine findings were adjudicated; two are product bugs in
the twin:

- the executed-count guard is `if count == 0 { Err(...) }`, which accepts a negative count, so a
  report could *shrink* `total_executed` while the Python publisher asserts `count > 0`;
- the sweep path never calls the file's own `required_inputs`, so the record's
  unavailable-vs-failed split is lost. This was demonstrated, not argued: in one isolated room,
  `python3 tools/checks.py` and the twin wrote the connected object over the same bytes and the
  delivered-shape object went from 71 groups / 4215 checks / 67 passed to 5 pass / 126 checks with
  64 zero-count groups that no row explains (record sha `acc7bc6517e9a590` → `85e487d2c6cb84e6`).

The name-to-report-file mapping was censused directly: `tools/package.rs` `report_pairs()` is
35 `EXPLICIT_PAIRS` + 22 `IDENTITY_PAIRS` + 14 `HYPHEN_PAIRS` = **71 rows for 71 distinct group
names**, joining with zero residue to the 71 names in `tools/checks.py`; `tools/package.py`'s
`pairs` covers only **57** groups, so the Python publisher cannot reproduce the delivered record it
claims to write. The single-quoted `checks.py` name list and the double-quoted `GROUPS` array in
`tools/harness.rs` were re-measured after the lane writes: 71 == 71, order-identical. The
verification census is still exactly 73 files / 8570 lines — at the ceiling pinned in
`tools/package.rs`, with no slack for the group currently being edited.

### A fleet hazard worth recording

`pgrep -f 'prompt-file …'` matches the launcher wrapper, not the model process. A `kill -9` on that
PID left the lane running, and relaunching into the same log path truncated the survivor's output.
Liveness must be read from `ps -eo pid,ppid,etimes,args`, not from a wrapper match.

## Historical baseline audit (original text)

# Release audit

## Current command and callback audit

Three defects were reproduced against the exact published controller from build 1793742554 and repaired in the shared Rust core:

- Workspace source switches now filter model operation references and structured declarations together. Re-enabling a configured source restores its permitted operations.
- A non-writing agent receives the executor's read-only operation families in both catalogs.
- Retired document, image and app-inspection callbacks preserve the exact preceding state before any project-source refresh. Explicit navigation continues to refresh current instructions.

All 30 connected regression groups pass, comprising 1,230 current checks and three separately identified historical compatibility results. All 194 selected Rust cases pass. The command group includes text and structured catalogs across all four provider formats, disabled and re-enabled sources, retained connection credentials, non-writing roles, actual reviewed file work, operation limits and scoped failures. The diagnostic group includes the five retired platform callbacks and explicit instruction refresh.

An additional live public HTTPS search passed through the actual review and callback path of an assigned child, retained source evidence and returned to the captured parent. The model replies in that test were controlled protocol fixtures; its search response was fetched live. Exact response byte count, digest and endpoint are recorded in verification/RESULTS.json. Optional live verification is available as `python3 verification/live_web.py --live`.

The intrinsic investigation method remains in ordinary command, child, baseline, probe, proposal, composition and compaction requests, including requests with optional guidance disabled. Actual file work also passes with an unavailable journal and absent development artifacts. A legitimate missing-file refusal leaves independent local work available.

Source inspection inventoried 288 distinct literal button events. The single-reference candidates use explicit parameterized dispatch for server-monitor panels, workspace selection, source switches and provider-format selection. Existing connected groups exercise the major navigation and action families. Installed Android execution supplies the remaining visual, touch, accessibility, lifecycle, IME, Keystore and installation acceptance.

The engineering roadmap remains open beyond device review: broader ISA/type/alias/inter-block analysis and structured decompilation; domain/process reconstruction; browser/language/compiler/runtime/inference coverage; provider authorization, ACP and storage extensions including SMB; resource/accelerator and recursive scheduling work. Configured live primary/child models, NAS/TLS and separate current-byte review retain their acceptance status. The record preserves these as open implementation or acceptance work, rather than treating this release as whole-project closure.

## Previous diagnostic release

This audit covers the diagnostic range and layout repair, all 138 compiled inputs, Android bindings, connected commands, storage, callback ownership, provider/delegation routing, encrypted-history flows, and package contents. The installed-device and live-service checks remain a separate acceptance step.

## Changes

- Exact averaging count: 1–128 recent requests, default 10, with 5/10/25/50 presets. Model filtering precedes the count. Applied settings survive state recovery.
- Successful measured requests supply latency means and time-weighted estimated throughput. Each gauge states its sample count. Success/error outcomes supply error rate. Live activity has a separate card.
- Charts, model statistics, request details and measurement history use the same selected window.
- Phone gauges use full-width compact cards. Tablet rows align at the top with baseline alignment disabled. Drawing width and height travel together to the Canvas binding; arc stroke bounds stay inside that drawing.
- Connection errors use neutral service wording. Diagnostic summaries describe measured actions directly. Both supplied backgrounds were visually inspected; neither has a visible watermark.
- Stale model progress, timer and failure deliveries preserve the pre-callback state exactly, including workspace namespace fields. Phone permission requests start from the explicit Setup control. An unapplied diagnostic count survives form capture and recovery.
- Unused native symbol/debug tables are stripped after linking. Runtime exports, both architectures and all product functions are retained. Required license notices remain packaged.

## Validation and dispositions

The public diagnostic workflow is exercised with actual controlled model replies, unequal request durations, saved-state recovery, pending live callbacks, cancellations, model filtering, invalid counts and an ordinary local file operation after refusal. The source runs without development reports or publication metadata in its workspace. Existing connected groups exercise local files, real loopback sockets, NAS-style HTTP, MCP discovery/calls, child returns, permissions, histories, exports and framing.

A wider run of the older application unit fixtures exposed failures already present in the previous signed source. Missing fixture workspace directories accounted for some failures. Other fixtures omit current operation tickets, call storage actions outside their current review flow, assume obsolete first-start permission behavior, or compare the complete state despite workspace namespace refresh. The stale model callback case exposed a real state-retention defect and was repaired in this release. Their current public-operation equivalents are exercised by the connected groups. These fixtures are preserved and their raw comparison results are recorded separately; their failures are not relabeled as successful checks.

An initial wording edit also omitted the literal HTTP scheme expected by the TLS guidance check. The final guidance includes both http:// and https:// and retains the configured endpoint unchanged.

The final selected suite passes 194 cases and all 30 connected groups pass. The broader core suite passes 398 cases with an explicit font and a fresh working directory containing the fixture’s expected parent directory. The application-only sweep passes 43 cases; its 12 remaining historical fixtures also fail against the preceding release and omit current callback/review prerequisites. Current equivalents pass in the connected groups. Exact counts, selected filters, comparison provenance and package hashes are recorded in verification/RESULTS.json. The previous-release controller was compiled from source build 1793733101, source checksum 06ffb8e78537914c932da623dc62048ab734bcc42dcbd9a01b0abd523bfad375. The older migration controller uses its actual saved core with a JSON-lines envelope only.

## Acceptance status

Owner execution and source review establish the checks recorded here. Current Android screenshot/installation/lifecycle/IME/Keystore validation, configured live models and NAS acceptance, and a separate reviewer execution are pending. The supplied screenshot is the pre-change reference. Drawing-bound checks and Java compilation are distinct from a current device screenshot.

The cumulative engineering roadmap remains preserved in project-progress.md and its referenced checkpoint. This release addresses the requested diagnostics and cleanup; future roadmap entries remain explicitly tracked.

## Package size

The signed APK is 5,986,322 bytes, compared with 8,394,770 bytes in the preceding release (28.7% smaller). Both ARM64 and x86-64 libraries remain included.
