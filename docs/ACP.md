# Workspace agent protocol

The standalone executable provides ACP v1 standard streams:

```bash
output/headless/RLLM --acp --root /absolute/selected/workspace
```

This interface connects the product's real read-only inspection and comparison engine to an agent client. It requires no model, broker, external agent executable or runtime SDK. The invoking account selects the root. Standard input and output carry newline-delimited UTF-8 JSON-RPC; diagnostics use standard error. The interface does not expose Android private files, execute supplied server commands, grant writes, or act as a general coding model.

Initialize with a positive integer `protocolVersion` and optional `clientCapabilities`/`clientInfo`. The current stable supported version is1; another requested version receives1 for agreement. Initialization does not start a session or execute a prompt. The advertised extensions are session list, in-process resume and close. Loading persisted conversations, external MCP peers, terminals, images, audio and embedded resources are not advertised. Unknown metadata grants no authority.

Create a session with `session/new`, the exact startup `cwd` and an empty `mcpServers` list. Each created session receives an opaque ID and available-command update. Prompts accept text and local file resource links. Text supports `/inspect relative/path`, `/inspect {"path":"relative/path","content":"complete_digest"}`, `/compare {"path":"first","other":"second"}`, or `{"tool":"inspect","args":{"path":"relative/path"}}`. A path omitted from the command can come from a local file link; comparison accepts two such links. URI escapes are decoded before root/path validation. Network URLs, other roots and parent traversal do not become file authority.

An accepted prompt starts an actual bounded worker and emits `tool_call`. The worker invokes the same guarded production reader as local UI, MCP and A2A operations. `tool_call_update` reports completed or failed execution, with actual output bytes and their digest. Agent message chunks contain the returned text. The prompt response follows those updates and contains the stable stop reason. Operation errors remain failed tool results rather than fabricated successful observations. Complete-file hashes require the explicit `complete_digest` argument; sampled observations retain their coverage limits.

`session/cancel` and `$/cancel_request` stop only their owned work. A cancelled prompt returns `cancelled` after the worker retires and publishes no agent observation. `session/close` first cancels an active turn, then closes after its final response. Another session remains usable. Resume refers only to an existing session in the same process and rechecks its selected directory; it does not fabricate or claim persisted history. EOF cancels owned work and ends the interface.

Bounds are8sessions, at most4simultaneous operations narrowed to usable CPU parallelism, one operation per session,16content blocks,8KiB text, two file links and64KiB input frames. Oversized frames drain without dispatching their tail, allowing the next frame to work. Returned text is limited to96KiB with an explicit truncation indication and actual full returned-output length/digest. Workspace readers retain their existing file, time, memory, cancellation, policy and no-follow descriptor bounds. Replacing the selected directory invalidates observations; a session ID or saved response never grants access to a replacement root.

The implementation follows the published [v1 initialization](https://agentclientprotocol.com/protocol/v1/initialization), [session setup](https://agentclientprotocol.com/protocol/v1/session-setup), [prompt lifecycle](https://agentclientprotocol.com/protocol/v1/prompt-turn), [tool updates](https://agentclientprotocol.com/protocol/v1/tool-calls) and [cancellation](https://agentclientprotocol.com/protocol/v1/cancellation) contracts. The protocol site currently labels v1 stable and v2 draft; the draft is not presented as production support. Specification review and actual subprocess checks establish the named capability scope, not acceptance by every third-party client.
