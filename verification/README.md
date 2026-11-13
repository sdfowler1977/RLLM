# Verification

Build the shared host controller with `bash headless.sh` using the tool locations in docs/BUILD.md. `bash tools/build-harness.sh` also compiles every native verification twin in `verification/*.rs` into `output/verification/`, and the runner prefers that twin over a Python leg of the same name. Run the connected groups from the project root:

```bash
output/verification/provider_process
output/verification/network_drive
output/verification/drive_ownership
output/verification/question_process
output/verification/history
output/verification/a2a
output/verification/acp
output/verification/oauth
python3 verification/binary_flow.py
python3 verification/pe_declarations.py
output/verification/commands
python3 verification/history_migration.py --previous-controller /path/to/prior-controller
python3 verification/hosted_phases.py
python3 verification/prompt_reference.py --previous-controller /path/to/prior-controller
output/verification/provider_evidence
output/verification/delegation_visibility
output/verification/local_feedback
output/verification/network_feedback
output/verification/proposal_feedback
output/verification/review_ownership
output/verification/task_ownership
output/verification/dav_listing
output/verification/server_admission
output/verification/http_framing
output/verification/platform_resolution
output/verification/flags_flow
output/verification/voice_background
output/verification/device_authorization
output/verification/carry_flow
output/verification/response_fields
output/verification/storage_etags
```

These owner-run groups exercise controlled provider/HTTP envelopes, actual shared-core routing and file workers, host sockets, retained records, native UI descriptions and guarded export/readback. Original assembled ELF/PE fixtures are compared with LLVM metadata and disassembly. Model visibility and delegation groups cover every provider format, exact/default selection, review identity, owned callback outcomes and parent-return retention. Provider-evidence groups cover automatic metadata receipts and refinement consumption.

Selected Rust cases compile from core/lib.rs with Rust1.99.1 using `--edition=2021 --test -C opt-level=0 -C codegen-units=16 -C linker-features=-lld`. Run each filter with `--test-threads=1`. The exact selected groups and outcomes are recorded in RESULTS.json.

The current prior-core reference is version1793395702 with a thin JSON-lines envelope adapter. Migration fixtures negotiate its requested MCP version. Request-size comparisons measure actual encoded request bytes.

Local-feedback checks execute actual failed file workers and follow their single-use callback through ordinary and child model requests. They cover ownership, permission changes, resource admission, repeated failures and fresh instructions.

Network-feedback checks follow current WebDAV HTTP, transport and parser failures through the requesting model, including create/revision readback and continuation admission.

The `http_framing` twin exercises real persistent loopback sockets through the shared response-framing API, including actual primary/child returns and WebDAV-to-model continuation. It covers coded response preservation, length/chunk/trailer boundaries, observed EOF, ownership and scoped resource/parser failures. The existing pure HTTP snapshot-framing cases run alongside network decoding cases.

The `platform_resolution` twin compiles and executes the production Java resolver/deadline binding using CLIENT_JDK. Controlled DNS lookup outcomes exercise occupied-slot admission, deadline expiry, lifecycle closure and recovery; hostname resolution then connects to real loopback sockets. Android call-site compilation uses SDK35.

Optional services: run the native twins `output/verification/media`, `output/verification/services`,
`output/verification/voice`, `output/verification/media_stream` and `output/verification/jobs`.
output/harness sweep includes these five groups alongside the original thirty. The current binary-flow compatibility group still requires the genuine revision-two controller; an unavailable comparator must be reported explicitly and never substituted with a current decoder or weakened reader fingerprint. Native microphone/playback and configured live service endpoints remain separate device acceptance.
