# Verification

Build the shared host controller with `bash headless.sh` using the tool locations in docs/BUILD.md. Run the connected groups from the project root:

```bash
python3 verification/provider_process.py
python3 verification/network_drive.py
python3 verification/drive_ownership.py
python3 verification/question_process.py
python3 verification/history.py
python3 verification/a2a.py
python3 verification/binary_flow.py
python3 verification/pe_declarations.py
python3 verification/commands.py
python3 verification/history_migration.py --previous-controller /path/to/prior-controller
python3 verification/hosted_phases.py
python3 verification/prompt_reference.py --previous-controller /path/to/prior-controller
python3 verification/provider_evidence.py
python3 verification/delegation_visibility.py
python3 verification/local_feedback.py
python3 verification/network_feedback.py
python3 verification/proposal_feedback.py
python3 verification/review_ownership.py
python3 verification/task_ownership.py
python3 verification/dav_listing.py
python3 verification/server_admission.py
python3 verification/http_framing.py
python3 verification/platform_resolution.py
```

These owner-run groups exercise controlled provider/HTTP envelopes, actual shared-core routing and file workers, host sockets, retained records, native UI descriptions and guarded export/readback. Original assembled ELF/PE fixtures are compared with LLVM metadata and disassembly. Model visibility and delegation groups cover every provider format, exact/default selection, review identity, owned callback outcomes and parent-return retention. Provider-evidence groups cover automatic metadata receipts and refinement consumption.

Selected Rust cases compile from core/lib.rs with Rust1.99.1 using `--edition=2021 --test -C opt-level=0 -C codegen-units=16 -C linker-features=-lld`. Run each filter with `--test-threads=1`. The exact selected groups and outcomes are recorded in RESULTS.json.

The current prior-core reference is version1793395702 with a thin JSON-lines envelope adapter. Migration fixtures negotiate its requested MCP version. Request-size comparisons measure actual encoded request bytes.

Local-feedback checks execute actual failed file workers and follow their single-use callback through ordinary and child model requests. They cover ownership, permission changes, resource admission, repeated failures and fresh instructions.

Network-feedback checks follow current WebDAV HTTP, transport and parser failures through the requesting model, including create/revision readback and continuation admission.

http_framing.py exercises real persistent loopback sockets through the shared response-framing API, including actual primary/child returns and WebDAV-to-model continuation. It covers coded response preservation, length/chunk/trailer boundaries, observed EOF, ownership and scoped resource/parser failures. The existing pure HTTP snapshot-framing cases run alongside network decoding cases.

platform_resolution.py compiles and executes the production Java resolver/deadline binding using CLIENT_JDK. Controlled DNS lookup outcomes exercise occupied-slot admission, deadline expiry, lifecycle closure and recovery; hostname resolution then connects to real loopback sockets. Android call-site compilation uses SDK35.
