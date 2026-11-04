# Local Agent2Agent access

RLLM exposes authenticated, read-only workspace observations through the existing native gateway and the headless executable. This is an opt-in local development HTTP interface, with A2A JSON-RPC wire versions1.0 and0.3. The shared Rust inspection/comparison engine runs without a model. It does not turn a protocol client into a phone-wide file controller.

On Android, open App settings → Network services → Phone API gateway, keep the listen address127.0.0.1, choose a bearer token of at least32 printable characters, enable the A2A workspace switch and start the gateway. The form shows the selected workspace. Its filesystem identity is captured when the gateway starts. Stop and restart to select a different workspace. The foreground notification reports actual service status and provides Stop. Existing model gateway routes keep their configured behavior; A2A workspace access cannot be enabled on the LAN address.

On Linux, build with headless.sh and run:

```bash
output/headless/RLLM --a2a --root /path/to/workspace --token-file /path/to/private-token --port 8080
```

The token file must be a private regular file, mode0600 or0400, with32–8192 printable ASCII characters and an optional final newline. It is never printed. The listener binds127.0.0.1; port0 selects an available local port and reports its URL to stderr. SIGINT/SIGTERM closes the listener and cancels owned work. This mode needs no provider connection. Stdout remains available for a calling controller.

Use `Authorization: Bearer TOKEN` for discovery and requests. GET `/.well-known/agent-card.json` returns the supported interfaces, read-only skills and bearer requirements. POST `/a2a` accepts application/json and fixed Content-Length. Set `A2A-Version: 1.0`; omitting the version defaults to0.3. An equivalent query parameter is supported. Patch versions do not change negotiation. Conflicting selections and unsupported versions return errors.

A current-version request is:

```json
{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"client-observation-1","role":"ROLE_USER","parts":[{"data":{"tool":"inspect","args":{"path":"sample.txt","content":"complete_digest"}}}]}}}
```

The result contains `task`; GetTask and CancelTask return a task directly. ListTasks filters retained tasks by context, status and inclusive status timestamp, sorts by latest status and pages an immutable short-lived snapshot. Include historyLength:0 to omit history; list results omit artifacts unless includeArtifacts:true. With configuration.returnImmediately:true, SendMessage returns before work completes; its default waits for a terminal result. Reusing an identical message ID returns the original retained task. Changed reuse is refused. Each task accepts one command; an existing or terminal task is never restarted by a new message.

Legacy0.3 uses message/send, tasks/get and tasks/cancel, user/agent roles, kind:data parts and a direct task result. Its blocking configuration defaults true. Current1.0 uses SendMessage, GetTask, ListTasks and CancelTask with the published enum names. Current cancellation is idempotent for an already cancelled task. Terminal completed/failed tasks cannot be cancelled.

Commands are inspect(path) and compare(path,other), with optional content:complete_digest. Paths must stay within the captured workspace; client fields cannot replace the root, grant permissions, request writes, execute code or fetch URLs. The existing descriptor and source-identity checks remain. Current user rules apply before work, throughout guarded reads and before result access; Ask requires the native review workflow and is refused by this raw interface. Rule changes during work prevent result adoption. Cached tasks and page tokens never grant access after revocation.

Task output contains the actual bounded engine text, its returned byte count and SHA256. At most24KiB of the returned text is retained; complete:false explicitly identifies a truncated result. That digest identifies the complete returned engine text, not the selected source file. Whole-file hashes require content:complete_digest. Directory and sampled-file reports retain their own coverage limits. Failed, timed-out or cancelled tasks do not publish an observation artifact. Cancellation has a separate owner and does not cancel/reset chat or another task.

The service retains at most8 tasks per interface,4 live interfaces and2 page snapshots per interface. Old terminal tasks are evicted before new task admission. Pages expire after60seconds. Idle terminal stores expire after30minutes. Stopped interfaces release task contents and are refused during the maximum in-flight request lifetime. Each task has a30second work deadline and a35second blocking-response deadline. Working response allowance is at most1MiB, narrowed by the captured native policy; concurrency respects the captured worker count and a global maximum4. The headless default is one worker, with at most8 local HTTP connections. At most one request waits synchronously for task completion, preserving a native HTTP worker for status/cancellation. Further blocking calls are refused before task creation; clients may use returnImmediately or retry after completion. Request limits are16KiB headers,1MiB gateway bodies and64KiB A2A bodies; task messages are8KiB. Bounds affect this service, never unrelated product operations.

The interface publishes its supported JSON-RPC methods and read-only observation commands in the authenticated agent card. Method negotiation follows that declared interface.

The wire shapes follow the official [A2A1.0.1 specification](https://a2a-protocol.org/v1.0.1/specification/), its version1.0 JSON-RPC binding, and the official [legacy0.3 specification](https://a2a-protocol.org/v0.3.0/specification/). Owner checks exercise real host HTTP and shared Rust/native gateway transitions. Recorded checks identify the exact host/controller paths exercised.

Workspace task completion and worker-slot release occur under the same task lock. The completed response therefore follows release of its actual execution capacity. Sequential fresh messages execute current source observations; identical message retries retain their original task and artifacts. Admission checks exercise256fresh tasks across both protocol versions with changed input bytes and matching source/output digests.
