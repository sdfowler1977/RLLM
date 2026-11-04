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

Run `python3 project/RLLM/tools/toolchain.py` from the workspace root to restore the pinned toolchain with verified downloads and contained extraction. Load `tooling/env.sh` after restoration completes.

## Complete APK

```bash
CLIENT_VARIANT=lab CLIENT_SIGNING_DIR=/private/rllm-identity CLIENT_VERSION_CODE=$(date +%s) bash package.sh
```

This builds both Android ABIs, JNI, Android resources and Java/DEX, packages the full APK, applies16KiB alignment and signs with the retained authorized key. The final RLLM.apk path is printed under the UTC build-start directory. CLIENT_UNSIGNED=1 allows local compilation checks and yields RLLM-unsigned.apk; it is not an installable update. The current source delivery contains its matching signed APK at release/RLLM.apk.

## Current signing identity

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

The tools directory retains the pinned toolchain restorer, connected-check runner, APK inspector, source packager and download readback verifier. Run tools/checks.py with the actual prior-controller reference, and use --units with the compiled selected Rust-check binary. Inspect the signed APK with tools/apk.py APK --version VERSION, package with tools/package.py --destination DESTINATION, then use tools/readback.py against the downloaded APK/source/checksums. These tools record the executed checks and exact file identities.

Delivery captures the inspected APK bytes, verifies the staged and published APK signatures with the retained certificate, and includes those exact bytes once in the source. Download readback repeats the signature and archive-integrity checks against the actual delivered APK.

Binary-flow compatibility checks use the actual preceding shared-core controller (1793592809, source SHA256353ad1a4d57684f17d5d26131c4a92ba5ff65fb921ecf5bdba8e80063e942c5b). Build its core/console.rs with the same host toolchain into output/pre-branch-controller, or pass --previous-flow-controller PATH to tools/checks.py. The older migration/request comparator is supplied separately through --previous-controller. These comparison binaries are development inputs.

Authentication-family compatibility also uses the actual revision-two controller1793593529 (source SHA256af219d4d2269f8f9ef0b9b9656106a8eca5dedbcebc5ef95047216de0c26b289). Build it into output/pre-auth-controller, or pass --previous-auth-controller PATH. The revision-one and revision-two checks retain the original capture bytes and meanings.

Each successful build records its exact source/resource input digests in its output directory. APK inspection verifies that inventory and the current inputs. Source packaging requires the archived build-input map to match that inspected APK, and reopens every source member. The compiled network-security XML and its manifest resource binding are inspected directly from the signed APK. These are development packaging checks.

When the original authenticated-history comparison executable is available, run the complete binary-flow group against it. For a change outside the historical graph reader, --auth-baseline PATH on tools/checks.py can retain the three already executed authenticated-history outcomes from the exact signed1793600489 source ZIP (SHA2563d51564e00233805e4cb6cbc0145d1227ed125b7545a036e4a9cb0c8aaebaa6a). The option verifies the archive and explicitly fingerprints the named decoder and representation reader modules recorded in RESULTS.json. Historical graph validation reuses retained instruction text and derives relationships through binary_evidence.rs/binary_flow.rs/control_flow.rs. The current instruction producer and command-completion paths are exercised separately. The three historical outcomes describe their original execution; the fingerprint comparison adds no new execution outcome. The check reports these outcomes separately from current executions. The actual older1793395702 controller can serve the revision-one flow comparison as well as migration/prompt checks; provide its path with --previous-flow-controller. This is development validation provenance.

A later verified current-source ZIP can carry the same three prior outcomes and their original reader-input hashes. Supply its independently checked source checksum with --auth-baseline-sha256 SHA256 alongside --auth-baseline PATH. The recorded original execution source and all unchanged-reader hashes remain in RESULTS.json; the current source and previous execution remain distinct provenance. This supports recovery from the single current source package.

The binary-flow group also runs the actual immediately published controller at output/published-controller (override with --published-controller PATH). Its freshly produced retained bytes pass through the current native view, ordinary model query and exact representation export. The current synchronous-journal group is verification/query_feedback.py. Development groups run sequentially with a 180-second bound per group; selected unit filters remain the explicitly recorded filters.

verification/carry_flow.py assembles original ADC/ADCS/SBC/SBCS32-bit and64-bit sequences and exercises exact operands, known/unknown carry, flags, coordinate identity, model queries, UI descriptions and exact export. The selected instruction unit filter also checks carry results and NZCV against independent wide-integer arithmetic and enumerated partially known operands.

The same assembled flow checks identical-register SUB/SUBS/SBC/BIC/ORN/EON/BICS and flag-driven selection. The unit oracle also verifies coordinate/unknown inputs,32-bit widths, zero-register meanings, unchanged flags and distinct-register controls.

The signing step explicitly emits v2/v3 signatures into a staging APK, verifies it, then writes the completed bytes sequentially with fsync into the delivery APK and compares both files. The actual persisted delivery APK is verified again before publication; later raw-download verification checks the same exact bytes. This preserves the complete signed ZIP and central directory across build-output storage.
