# Tool feedback and web results

A proposal must be the entire model response as bare JSON. A recognized tool name plus an args object inside prose is flagged as a possible formatting mistake, never extracted for execution. Quoted/fenced/inline-code examples, nested configuration data, unknown tool names and user messages are excluded.

The raw reply is retained within existing history limits. Open Conversation actions → Request format correction for one explicit corrective model request. It uses the unchanged originating connection and agent/task context. Nothing is sent automatically; any corrected valid proposal still opens the usual approval review. If correction is still malformed, it stops. Capacity limitations are explicitly reported.

Tool permissions and captured identity are checked again at approval. Web callbacks bind to the exact current ticket and conversation. Rejection, cancellation, startup/transport failure, a successful fetch and a failed model follow-up remain distinguishable. Scoped failures retain bounded diagnostic evidence without claiming a successful page read or automatically replaying the operation.

Current web_read is static HTTP content extraction. It does not execute webpage JavaScript, capture rendered screenshots, click, type, scroll or bypass login/CAPTCHA challenges. Rendered browsing remains separate implementation work.

Validation covers the shared headless event interface and real loopback HTTP: malformed reply → explicit correction → approval → exactly one page request → captured evidence → model continuation. Package rebuild checks do not replace installed Android/OEM/device or live-site validation.
