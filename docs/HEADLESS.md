# Standalone operations

Build the Rust executable with `CLIENT_RUST=/path/to/toolchain bash headless.sh`. The executable uses the application core and operation envelope shared with the Android JNI entry point. The standalone target is Linux x86-64; the Android package contains ARM64 and x86-64 libraries.

`output/headless/RLLM --jsonl` reads one UTF-8 JSON operation per line and writes one JSON envelope per line. Validation, current process-held user rules, worker ownership and results are shared with `client_process`. UI events return state, UI descriptions and platform effects. Trusted controllers handle these effects, persist state securely and return authentic callbacks. WebDAV network effects carry drive_ticket in addition to purpose; return that exact ticket on network_result and failure, together with any child_ticket. Tickets are single-use process-held ownership, not permission grants. Missing/old/replayed owners are inert. Resume/cancel revokes pending network ownership; restoring a saved state does not resume network work. Optional authorization can restore only its pending browser proof and wait for the matching return, without replaying an exchange or model request. Controllers must encrypt saved application state, including credentials and pending browser proof.

```json
{"op":"tool","root":"/path/to/workspace","tool":"inspect","args":{"path":"sample.txt"}}
```

Each input line supports up to8MiB. Framing errors return a local error envelope, and processing continues with the next complete line. EOF ends the executable.

The response_details operation keeps provider text separate from an optional proposal object. Completed structured calls normalize to {tool,args}; an interrupted reply never supplies a proposal. This operation decodes data and executes no tool. Application events route normalized proposals through the same current permissions, user review, child ownership and tool-result conversation as text proposals. The older response operation remains text-only.

# MCP standard streams

`output/headless/RLLM --mcp --root /path/to/workspace` provides read-only `inspect` and `compare` tools over newline-delimited JSON-RPC standard streams. The selected existing workspace supplies local read authority under the host account. Its filesystem identity is pinned for the session.

The lifecycle includes initialize, initialized notification, ping, tools/list and tools/call. Supported revisions are2025-03-26,2025-06-18 and2025-11-25. For an unknown requested revision, initialization returns the latest supported alternative for client agreement. Tool calls carry request IDs. Protocol errors use JSON-RPC errors; file-operation errors use `isError:true`. Response frames use stdout, and logs/startup errors use stderr.

Arguments include relative `path`, comparison `other` and optional `content:"complete_digest"`. The shared analyzers report actual file/directory observations and coverage. Whole-file digests read complete selected regular files; prefix samples retain their observed byte lengths. Comparisons read the two sources sequentially.

File operations use nonblocking no-follow descriptors and revalidate root, component and file identities. Workspace paths support up to512components within1024bytes, with bounded retained descriptors. Operations run synchronously. Closing the process ends the read-only session.

JSON-lines controllers can publish current rules through the harness-policy operation. MCP uses its fixed read-only tool surface and selected-root authority. Each host process maintains its own state and policy context.

# MCP client connections

The app's MCP client negotiates March,June andNovember2025 revisions. Initialization proposesNovember; subsequent HTTP requests carry the selected protocol header. Tool catalogs, text/media/resource/structured results, control replies and cleanup use the negotiated revision. March JSON-RPC batches retain their version-specific handling.

Server listening reports received notification/request counts. Empty SSE data events preserve cursors. Explicit Resume uses an acknowledged printable cursor for up to4resumptions within10minutes, and honors server retry timing.

# A2A local HTTP

The same executable now provides versioned read-only A2A tasks with --a2a, a selected root and a private bearer-token file. See [Local A2A access](A2A.md) for Android controls, protocol examples, bounds and shutdown behavior.

# Workspace agent sessions

`output/headless/RLLM --acp --root /absolute/workspace` provides stable ACP v1 standard streams with actual shared-engine inspection/comparison, session updates, list/resume/close and scoped cancellation. It does not launch another agent or require a model. See [workspace agent protocol](ACP.md) for the supported commands, capabilities, safety boundaries and resource limits.
