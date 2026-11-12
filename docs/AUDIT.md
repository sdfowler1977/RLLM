# Linux host, packaging and verification audit (2026-11-10, current through 2026-11-12T01:41Z)

This section is the current audit. The sections below it describe earlier signed candidates and keep
their original counts and wording.

## What was measured

- Core unit suite through the sanctioned runner (`python3 tools/checks.py --all-units`): **517/517
  cases plus 55 isolated application cases, exit 0**. Measured on both toolchains: the drifted
  `rustc 1.95.0` build (`output/units`) and the pinned `rustc 1.99.1` build (`output/units-pinned`,
  built from `tooling/env.sh`, log `output/units-pinned.log`) give the identical tally. A bare
  `rustc --test core/console.rs` from the
  repository root reports 503 passed / 14 failed; every one of those 14 is a runner gap, not a product
  defect — the fixtures use the relative scratch root `toolchain/check-workspace`, which the bare run
  never creates, and `capture_job::resource_checks` needs `CLIENT_CAPTURE_TEST_FONT` pointed at a real
  TrueType font. `tools/checks.py` now supplies that font itself instead of leaving the batch red.
- Connected groups through `tools/harness.rs`: **64 passed, 0 failed, 4 unavailable of 68 groups**,
  census clean, exit 0 (2026-11-11T21:20Z; a second full sweep wrote a byte-identical record). The runner
  reports a missing development input as `UNAVAILABLE` instead of charging it to the product, and a group
  whose own source guards an optional reference now runs with that comparison recorded as an unexercised
  leg instead of being blocked to a recorded `0`.
- `verification/RESULTS.json` now carries the current census: **68 groups, 64 passed, 4,155 checks
  executed**, with the 4 unavailable groups recorded as 0 plus their missing inputs and a
  `zero_count_disclosure` line (earlier revisions of this document described the record as holding
  65 groups / 4,228 checks — the previously delivered candidate, before `state_sealing` and
  `desktop_runtime` were added — then 67 groups / 4,016 checks before `turn_background` was added,
  4,041 checks before that group gained its turn-lifetime checks, and 4,044 checks before
  `apk_alignment` gained its artifact-resolution checks; 65 passed / 4,142 followed the rebuild of
  `output/published-controller`, and the current 4,155 comes from `prompt_reference` executing 52 checks
  while the off-host signed APK blocks `apk_alignment` and `turn_background`).
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
`tools/readback.py` verifies the companion by name and byte identity rather than by a release path.
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
(`tools/package.py:117`) are not on this machine, so `tools/readback.py` has nothing to verify until a signed
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
   (`core/harness.rs:396-434`), is capped at **two** rounds (`core/improvement.rs:109-115`),
   scores only user phrases / JSON fields (`:36-67`) and keeps a candidate on a raw passed-count
   gain (`:120-124`). Missing: edit budget, edit history, stall exploration, leakage critic, noise
   floor, cost-aware acceptance, pruning, held-out/OOD split, parallel candidates, worktree
   candidates, resumable JSONL run with a STOP file.
3. **`GAP3-HARNESS2-TWO-LEVEL-RECURSION`** — PPC and the lessons store exist; no candidate pool,
   no composition from a pool, and no persistent improvement library that a later task reads back.
4. **`GAP4-REA-FEATURE-REBUILD`** — `reconstruct` copies bytes, exports a retained representation,
   or writes a structural spec; its own review warning says it "does not recreate original source
   bytes or behavior". No JS/Electron, .NET/CIL or HAR/SAZ analysis in `core/`.
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
so the source bundle deliberately carries no APK — `tools/readback.py` asserts `not apks` in that branch —
and the companion bytes (`RLLM__Candidate-Application__2026-11-10__00-23-57Z.apk`, 6,932,498 bytes,
`86b7d741…`) exist nowhere on this machine, which is also why the readback leg cannot be run here
(`output/checks/packaged.json` is absent too). Of the 162 pinned build inputs, 136 match the working tree exactly and 26
have drifted (`AndroidManifest.xml`, `platform/com/local/client/Activity.java` and 24 `core/*.rs` files —
`application`, `capture_file`, `claim_records`, `comparison`, `control_flow`, `css_cascade`,
`css_selectors`, `delegation`, `dex`, `dex_code`, `directory_observation`, `elf_observation`, `harness`,
`hosted_search`, `image_png`, `instruction`, `investigation`, `lib`, `page_capture`, `page_layout`,
`pe_observation`, `search`, `text_encoding`, `web_document`); 0 recorded inputs are missing from the tree.
Three files the publisher would now enumerate are not in the manifest at all: `core/desktop.rs`,
`core/state_crypto.rs` and `platform/com/local/client/TurnService.java`. So `tools/package.py:106`
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
