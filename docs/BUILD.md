# Build and signing

## RLLM package

`package.sh` defaults to `com.local.client.lab` with the RLLM launcher name. The user authorized the retained replacement signing identity. Both preflight and the signed APK certificate are checked against that fixed identity. The current update retains the existing installed package.

Set CLIENT_SIGNING_DIR to the private directory containing rllm.p12 and rllm.password, alias rllm. These are private build inputs and must not enter the source or APK. Alternatively set CLIENT_KEYSTORE, CLIENT_KEY_ALIAS and CLIENT_KEY_PASSWORD explicitly. The older main package retains its own original signer.

## Development tools

Linux x86-64 host: Rust1.99.1, JDK17.0.21+9, Android platform35 revision02, build-tools35.0.0 and NDKr28c(28.2.13676358). Install Rust standard libraries for x86_64-unknown-linux-gnu, aarch64-linux-android and x86_64-linux-android. TOOLCHAIN.json records official downloads and checksums.

```bash
export CLIENT_RUST="/path/to/rust"
export CLIENT_JDK="/path/to/jdk-17"
export CLIENT_ANDROID="/path/to/platform/android-35"
export CLIENT_TOOLS="/path/to/buildtools/android-15"
export CLIENT_NDK="/path/to/android-ndk-r28c"
export LD_LIBRARY_PATH="$CLIENT_RUST/lib:$CLIENT_TOOLS/lib64:$CLIENT_JDK/lib:$CLIENT_JDK/lib/server"
```

Run `bash project/RLLM/tools/build-tools.sh` from the workspace root, then `project/RLLM/output/tools/toolchain`, to restore the pinned toolchain with verified downloads and contained extraction. Load `tooling/env.sh` after restoration completes.

## Complete APK

```bash
CLIENT_VARIANT=lab CLIENT_SIGNING_DIR=/private/rllm-identity CLIENT_VERSION_CODE=$(output/tools/apk --next-version-code --package com.local.client.lab) bash package.sh
```

This builds both Android ABIs, JNI, Android resources and Java/DEX, packages the full APK, applies16KiB alignment and signs with the retained authorized key. The final RLLM.apk path is printed under the UTC build-start directory. CLIENT_UNSIGNED=1 allows local compilation checks and yields RLLM-unsigned.apk; it is not an installable update. The matching signed APK is delivered separately from the source ZIP.

The release number is not read from the wall clock. `output/tools/apk --next-version-code --package PACKAGE` returns the first version code strictly above the release floor of that package, and `--package` may be left out to ask for the floor of the whole tree. The floor of a package is the join of the release numbers the retained artifacts of that package carry, the distributed baseline the publication record holds for that package — the `distributed_baselines` ledger, and the scalar `distributed_baseline` for the package the record names — and the version the record published last. The floor therefore belongs to the tree and not to the host: two hosts that hold the same tree derive the same number, while a host whose tree holds a newer artifact or a newer record derives a different one. The tool reads the tree it is asked about: `--root DIR` names it, and without it the tool resolves the project root from its own location. A code at or below the floor is refused by name, with the code, the floor and the package named, before any toolchain or build directory is touched. `package.sh` reads the same floor through the same tool and for the package the variant publishes, so the route and the tool can never hold two different floors, and it advances the record's floor of that package when the build succeeds.

## Current signing identity

Build intermediates do not establish a package's release floor. Package evidence
comes from the publication record, `APK/`, and final `RLLM.apk` or
`RLLM-unsigned.apk` files directly in an `output/` build directory. When a package
has no such evidence, both `--release-baseline --package PACKAGE` and
`--next-version-code --package PACKAGE` use the conservative whole-tree floor;
the latter returns that floor plus one. This prevents an intermediate manifest
with the initial code 1 from resetting an unpublished package's floor.

Certificate SHA256:

```text
ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5
```

Reuse the current private signing directory for each build so subsequent APKs retain this identity.

The script reads the password as data, verifies the fixed certificate before compiling, checks that build inputs remain unchanged throughout compilation, signs, verifies v2/v3 signatures and the final certificate, then checks16KiB ZIP alignment. A source change stops that build before signing/delivery and requires a fresh build; no such source/checkpoint dependency is added to application execution. Verify ZIP CRC, both ABI libraries and ELF LOAD alignment as part of delivery. The source catalog records packaged file identities.

## Standalone target

```bash
CLIENT_RUST=/path/to/toolchain bash headless.sh
output/headless/RLLM --jsonl
output/headless/RLLM --mcp --root /existing/workspace
```

The host build uses `-C linker-features=-lld` and produces the shared-core controller at output/headless/RLLM. HEADLESS.md describes framing, selected-root authority, protocol versions and tool arguments.

## Development recovery and delivery

The tools directory retains the pinned toolchain restorer, connected-check runner, APK inspector, source packager and download readback verifier, plus the native packaging tools built by tools/build-tools.sh from tools/alignment.rs, tools/readback.rs, tools/checkpoint.rs, tools/signature.rs, tools/apk.rs, tools/toolchain.rs, tools/gate.rs and tools/package.rs into output/tools/. Run output/harness sweep with the actual prior-controller reference, and use output/tools/gate --units with the compiled selected Rust-check binary. Inspect the signed APK with output/tools/apk APK --version VERSION. The native publisher is output/tools/package --destination DESTINATION. Pass --candidate only for the single retained binary_flow gap. A dry run is output/tools/package --destination DESTINATION --dry-run. It performs the same reads, digest checks and inventory assertions, prints the pinned build_inputs and files censuses as match, drift and absent, prints the path and byte count of each artifact it would write, and writes nothing except the destination directory. tools/package.py remains the Python publisher until a signed candidate is published with the native tool. Then use output/tools/readback against the downloaded APK/source/checksums. These tools record the executed checks and exact file identities.

Work-in-progress is checkpointed with `output/tools/checkpoint DESCRIPTION` — letters and single hyphens only, and a bad description is refused before any directory is created — which writes `RLLM__DESCRIPTION__<UTC stamp>.zip` into the `recovery/` directory beside the project root. That target is environment-owned: on a host where it is not writable the tool prints `PermissionError: [Errno 13] Permission denied: '<path>'` and creates nothing, and it is not a release artifact in any case. The unit gate used by the product's own self-rebuild arm is the same binary a developer can run: `output/tools/gate UNITS` prints `Rust cases passed: N/M; isolated application cases: K` and writes `output/checks/full-units.json`.

Delivery captures the inspected APK bytes, verifies the staged and published APK signatures with the retained certificate, and records the companion APK’s exact length and digest in the source manifest. Download readback repeats the signature and archive-integrity checks against the actual delivered APK.

Binary-flow compatibility checks use the actual preceding shared-core controller (1793592809, source SHA256353ad1a4d57684f17d5d26131c4a92ba5ff65fb921ecf5bdba8e80063e942c5b). Build its core/console.rs with the same host toolchain into output/pre-branch-controller, or pass --previous-flow-controller PATH to output/harness sweep. The older migration/request comparator is supplied separately through --previous-controller. These comparison binaries are development inputs.

Authentication-family compatibility also uses the actual revision-two controller1793593529 (source SHA256af219d4d2269f8f9ef0b9b9656106a8eca5dedbcebc5ef95047216de0c26b289). Build it into output/pre-auth-controller, or pass --previous-auth-controller PATH. The revision-one and revision-two checks retain the original capture bytes and meanings.

Each successful build records its exact source/resource input digests in its output directory. APK inspection verifies that inventory and the current inputs. Source packaging requires the archived build-input map to match that inspected APK, and reopens every source member. The compiled network-security XML and its manifest resource binding are inspected directly from the signed APK. These are development packaging checks.

When the original authenticated-history comparison executable is available, run the complete binary-flow group against it. For a change outside the historical graph reader, --auth-baseline PATH on output/harness sweep can retain the three already executed authenticated-history outcomes from the exact signed1793600489 source ZIP (SHA2563d51564e00233805e4cb6cbc0145d1227ed125b7545a036e4a9cb0c8aaebaa6a). The option verifies the archive and explicitly fingerprints the named decoder and representation reader modules recorded in RESULTS.json. Historical graph validation reuses retained instruction text and derives relationships through binary_evidence.rs/binary_flow.rs/control_flow.rs. The current instruction producer and command-completion paths are exercised separately. The three historical outcomes describe their original execution; the fingerprint comparison adds no new execution outcome. The check reports these outcomes separately from current executions. The actual older1793395702 controller can serve the revision-one flow comparison as well as migration/prompt checks; provide its path with --previous-flow-controller. This is development validation provenance.

A later verified current-source ZIP can carry the same three prior outcomes and their original reader-input hashes. Supply its independently checked source checksum with --auth-baseline-sha256 SHA256 alongside --auth-baseline PATH. The recorded original execution source and all unchanged-reader hashes remain in RESULTS.json; the current source and previous execution remain distinct provenance. This supports recovery from the single current source package.

The binary-flow group also runs the actual immediately published controller at output/published-controller (override with --published-controller PATH). Its freshly produced retained bytes pass through the current native view, ordinary model query and exact representation export. The current synchronous-journal group is verification/query_feedback.py. Development groups run sequentially with a 180-second bound per group; selected unit filters remain the explicitly recorded filters.

verification/carry_flow.py assembles original ADC/ADCS/SBC/SBCS32-bit and64-bit sequences and exercises exact operands, known/unknown carry, flags, coordinate identity, model queries, UI descriptions and exact export. The selected instruction unit filter also checks carry results and NZCV against independent wide-integer arithmetic and enumerated partially known operands.

The same assembled flow checks identical-register SUB/SUBS/SBC/BIC/ORN/EON/BICS and flag-driven selection. The unit oracle also verifies coordinate/unknown inputs,32-bit widths, zero-register meanings, unchanged flags and distinct-register controls.

The signing step explicitly emits v2/v3 signatures into a staging APK, verifies it, then writes the completed bytes sequentially with fsync into the delivery APK and compares both files. The actual persisted delivery APK is verified again before publication; later raw-download verification checks the same exact bytes. This preserves the complete signed ZIP and central directory across build-output storage.

Before signing, the build runs tools/build-tools.sh to compile the native packaging tools from tools/alignment.rs, tools/readback.rs, tools/checkpoint.rs, tools/signature.rs, tools/apk.rs, tools/toolchain.rs and tools/gate.rs into output/tools/ — each source is built twice, the binary and then a --test build that this script executes, so a tool's declared unit cases must actually run and the executed count must equal the declared count — and then runs output/tools/alignment to stream and hash every original and aligned APK member, verify lengths/CRC and check official ZIP alignment. A truncated or changed alignment output is retained for inspection and retried once. A second failure stops this build before signing. The aligner source tools/alignment.rs is included in the frozen build-input inventory, and the tool build sits inside that same input-drift window, so the binary that aligns this APK is built from the bytes the inventory records. This build-time protection adds no application runtime dependency.

Tool recovery also verifies persisted compiler archives after a parent-owned sequential write, file fsync, close and rename, then verifies them again before extraction. One bounded download retry is allowed. Rejected cache bytes are retained under distinct names; failed or truncated downloads cannot be reported as restored tools. This repairs an observed recovery where two target standard-library archives became truncated after the old downloader reported success.

The current check runner contains 72 ordered groups. Packaging compares the complete ordered runner inventory and every report mapping rather than an older fixed group count. New transport groups are included in counts and retained reports. Published-controller provenance must name the actual verified source archive and hash the actual executable; it must not describe a different published build. Missing historical comparison results remain blocking or explicitly unavailable under the existing candidate policy; they are never replaced by current-controller results. These are build-time controls only.

The credential-rotation group exercises malformed refresh responses through actual local HTTP authorization/storage peers. Its preceding-controller reproduction is recorded separately; source token validation never depends on those development results.

The storage_owner group exercises same-destination stale token generations through actual browser, device, refresh and pre-send-failure callbacks. It records 43 checks and actual HTTP exchanges separately; no device or independent acceptance is implied.

The mcp_catalog development check drives actual paginated discovery through a local MCP HTTP peer. It checks failed refresh preservation, successful atomic publication, same-page and cross-page duplicate names, case-distinct names, legacy selection safety, prior-catalog collisions, connection switching and stale callback refusal and interruption cleanup.

The mcp_images development check exercises reviewed calls to a local MCP HTTP peer, exact PNG and JPEG adoption, raw details, paging, refused inputs, and the existing media preview verification and export contracts. Installed Android preview and export require device execution.

The mcp_continuation development check connects loopback model and MCP peers through the real primary and delegated call paths. It covers model payloads, conversation and workspace ownership, duplicate and cancelled callbacks, local retention capacity and exact media preview verification.

The mcp_history development check exercises exact-entry historical image review, stale selection, direct requests, delegation and compaction through a local model peer. It checks original history retention and scoped handling of changed entries.

The mcp_catalog_events development check uses real loopback MCP negotiation, standalone and correlated POST streams, cursor resume, March SSE batches and paginated refresh. It covers per-connection refresh indications, stale callbacks, duplicate correlated replies, explicit HTTP framing, malformed header and chunk metadata before control dispatch, split chunk delimiters, fragmented valid trailers, malformed trailers received with a control event, prior GET progress preservation, overlong unterminated chunk headers, split CRLF at the exact 8 KiB header limit, and retention of the last complete catalog.

The http_framing development check drives owned responses through real persistent loopback sockets. It splits CRLF at the exact 8 KiB chunk-size limit for delegated model responses and checks terminal chunks at the same limit. It also splits CRLFCRLF at the 64 KiB final and informational header limit, drives persistent model and WebDAV responses, and verifies that overlong metadata retires only the affected response session. Incremental model and MCP checks cover a final header at the same limit after informational headers, split delimiter delivery, and actual model output or an acknowledged MCP ping and tool result. Chat and local stream cases check exact-length split replies, zero and nonzero overflow, malformed and duplicate lengths, conflicting chunked framing, unsupported and duplicate encodings, valid chunked identity responses, and preservation of text from earlier valid reads.

The signed APK is distributed alongside the source ZIP. The manifest binds the companion filename, bytes and SHA-256; the complete validation report is stored once in `verification/RESULTS.json`. For `output/harness sweep` or the standalone APK alignment group, set `RLLM_CHECK_APK` to the absolute path of the signed APK. Legacy sources with an embedded APK also remain verifiable.

Raw historical unit logs and superseded point-in-time check reports remain in the verified build 1794225793 source backup. Current development checks and the historical audit summaries remain available in this source. The Android package does not include the development checks or reports.

For the complete Rust suite, run `CLIENT_CAPTURE_TEST_FONT=/absolute/path/to/font.ttf output/tools/gate /absolute/path/to/test-binary`. Each application fixture runs in its own process and workspace, and the other modules run serially. This keeps process-held tickets from an earlier fixture out of the next one.

The HTTP framing group checks that chat and local streams validate complete chunk delimiters and trailers before text adoption or completion, reject bodies on HTTP 204, and enforce the shared 16 KiB trailer limit on owned responses. The MCP catalog-event group exercises a real server ping whose length-framed acknowledgment follows an informational response. The hosted phases group checks both isolated evaluation tasks, local-only expected results, interrupted work and the all-task lesson policy.

Connected checks exercise individual JSON field scores, preservation of earlier passes across rounds, verifier isolation, and WebDAV namespace scoping through the listing and file-open entry points.

Connected coverage includes real over-limit media downloads, completion and reuse attempts on failed tickets, and local JSON repair in refinement and held-out checks.

Media transfer checks also cover failed import chunks followed by finish, and a mutated selected file followed by a repeated upload read.
