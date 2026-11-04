# Tool feedback and web results

A proposal must be the entire model response as bare JSON. A recognized tool name plus an args object inside prose is flagged as a possible formatting mistake, never extracted for execution. Quoted/fenced/inline-code examples, nested configuration data, unknown tool names and user messages are excluded.

The raw reply is retained within existing history limits. Open Conversation actions → Request format correction for one explicit corrective model request. It uses the unchanged originating connection and agent/task context. Nothing is sent automatically; any corrected valid proposal still opens the usual approval review. If correction is still malformed, it stops. Capacity limitations are explicitly reported.

Tool permissions and captured identity are checked again at approval. Web callbacks bind to the exact current ticket and conversation. Rejection, cancellation, startup/transport failure, a successful fetch and a failed model follow-up remain distinguishable. Scoped failures retain bounded diagnostic evidence without claiming a successful page read or automatically replaying the operation.

web_read defaults to static HTTP content extraction. Explicit scripts:inline evaluates the supported bounded inline-program subset through the existing reviewed Rust page runtime; optional capture creates actual PNG pixels with its supported renderer. Same-origin resource capture is optional and bounded. The selected script/resource settings control page evaluation and rendered capture. See [static capture](STATIC-CAPTURE.md).

Validation covers the shared headless event interface and real loopback HTTP: malformed reply → explicit correction → approval → exactly one page request → captured evidence → model continuation. Recorded results identify the shared controller and host HTTP paths exercised.

## Proposal refusals

A model-proposed operation with a disabled permission or invalid arguments returns its actual rejection to the requesting model through the ordinary command flow. The conversation retains the original proposal and diagnostic. A corrected operation follows the existing review and permission checks. Standalone tasks retain their assigned route, queued reviews retain their original owner, and child refusals return to the captured parent. Repeated errors share the bounded operation-feedback allowance; a fresh user instruction starts a fresh allowance.

The current operation review retains its exact arguments and owner through later model replies. A conflicting proposal returns the current review status to the requesting model. Complete or dismiss that review to choose a replacement operation. Manual review arguments stay in the local review until the user approves the operation.

During an active agent task, a newly submitted composer message stays as a draft. Native Status shows the owned task and its review. The task keeps its assigned connection and captured parent through actual result or cancellation. Send the preserved draft after that task settles, or cancel the current task explicitly to begin a new instruction.

Retry retains the task's assigned connection and captured context. When an operation awaits review, Retry shows that review's status; the reviewed operation continues through its existing approval controls.

An active task stays in its original conversation through New conversation, regeneration, branch acceptance and History selection. Finish or explicitly cancel that task before changing conversations. Browsing History and returning to the current chat remain available.

Rejecting a child's proposed operation stops that child task. Status and the following model request retain the actual interrupted transition and the rejected operation, alongside the original task context.

Approved observations, hypotheses and experiment queries return their actual journal result or operation error to the requesting model through the same bounded feedback path. An unsuccessful query retires its pending approval and retains the exact diagnostic. Successful local changes are committed before model continuation; a paused continuation keeps the saved result, and Retry uses the assigned task connection. Child reports retain their agent and task identity and return to the captured parent.

Current tool permissions and captured author identity are checked before mutation. A changed author or connection keeps the original unexecuted review. A journal read denied by current source permissions returns the denial as result data; the retained source content stays behind the same access checks. Failed queries preserve prior standing approval settings.
