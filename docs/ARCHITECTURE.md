## Question process

The exact current source/build-input inventory is recorded in PUBLICATION-MANIFEST.json. `core/inquiry.rs` is owned by the existing engine; application dispatch, the existing hypotheses operation and task/provider lifecycle use it directly. All four existing provider formats share process guidance and owned tool completions. Without a model, use Investigation → Question and design process. No added runtime dependency. See [process guide](QUESTION-PROCESS.md), [current scope](STATUS.md) and [release notes](../release/NOTES.md). Earlier instructions below remain preserved; latest release facts govern this version.

## Current access and inspection controls

See [Access and data controls](ACCESS-AND-DATA.md) for broad shared-storage, selected-folder and disabled modes; explicit selected-app metadata reads; exclusions; and model data destinations. App inspection does not access private app files or prove spying. Leading malformed tool-call markup is a scoped error, with raw history retained and no execution/replay. This section supersedes older folder-only browsing descriptions below.

# Architecture

## Rust application core

`core/lib.rs` exposes a C interface: `client_process` accepts UTF-8 JSON requests and returns an allocated JSON response; `client_free` releases that allocation. The JNI bridge wraps it for Android.

`core/application.rs` owns application events and state, conversation routing, request tickets, configuration and UI descriptions. Supporting modules implement provider formatting, generation controls, bounded streams, agent tasks, delegation, investigation, project transfers and metrics. The core returns effects describing work the platform must perform rather than embedding Android UI calls.

## Android platform

`platform/com/local/client/Activity.java` renders native Android views, collects user events, performs platform networking/document operations, and returns scoped results to Rust. `Diagram.java` draws monitor geometry. `ExportStore.java` and `TransferStore.java` manage saved transfer data. `Execution.java` and `Gateway.java` bind Android services. `platform/jni/bridge.c` is the native bridge.

App state is encrypted using an Android Keystore-backed key and AES-GCM. This is local app-state protection; it does not make transmitted prompts private from the selected model provider. HTTP and HTTPS behavior follows the configured endpoint. The application does not require a browser/WebView UI or bundle a language model.

## Requests and observations

Each model operation gets a ticket. Late callbacks cannot simply attach to the next request. Streaming progress is provisional until completion; interruptions retain partial text with its status. Rust decoders support local NDJSON plus chat-completions, Messages and Content SSE. HTTP integrity and the selected protocol completion marker are both required. Decoder/session bounds prevent unbounded accumulation, and visible text is separate from hidden reasoning and provider-reported token metadata. Request history holds a bounded set of observations.

`core/throughput.rs` derives the marked local estimate from received response text and elapsed request time. `core/monitor.rs` presents it alongside duration, first-text, character-rate and error information. Provider token usage and optional remote hardware metrics are separate sources.

## Tool boundary

Profiles specify permitted tools, reusable skills and callable subagents. The app reviews proposed operations and executes the accepted operation through the appropriate owned core/platform implementation. A model's prose, tool result or saved lesson is not a permission grant. Current refinement uses bounded baseline/proposal/probe/composition/adoption stages, while delegation captures parent and child context separately. Explicit refinement can snapshot up to four recent global adopted lessons as bounded, untrusted proposal/composition examples. The user can opt out; historical examples never grant tools or override protected instructions.

Malformed response recovery keeps raw commentary visible. A single unquoted, unfenced, top-level request in the current reply is data for an explicit Review proposed request action. That action verifies current originating profile, agent/task, permissions, operation limits and arguments, then stages the ordinary review without dispatch. Its one-time approval marker prevents saved-rule and matching-rule execution. Example/nested/ambiguous requests and hidden reasoning cannot acquire authority through this path. Response details retains the raw text and original branch action.

Built-in search may explicitly select the alternative endpoint for one newly reviewed exact-query attempt. That selection is captured in the operation arguments and displayed destination. It does not repeat the primary source, rewrite a query, change saved settings or borrow custom credentials. Custom endpoints refuse this built-in-only selection.

## Platform boundaries

The Rust core offers reusable domain logic, not an already portable end-user application. Android services, Keystore, document URIs, UI lifecycle and platform network bindings need replacements on another operating system. The archive implementation includes Linux/Android-specific descriptor bindings. See [the iOS assessment](IOS.md).

## Native investigation execution

`core/exploration.rs`, `core/comparison.rs` and `core/binary_observation.rs` implement original Rust structural document observations, held-file byte comparison and selected binary envelopes. `investigation.rs` connects these readers to actual file input. Shared application dispatch and harness result paths carry observations into ordinary chat and recursive tasks. Investigation preferences add task guidance, and configured remote MCP connections provide tool catalogs and reviewed calls. [Question and design process](QUESTION-PROCESS.md).

## Response framing

network.rs owns incremental HTTP response framing and bounded, owned response sessions. transfer.rs reuses it for verified snapshot streams. The Wire.java platform binding supplies Android InputStream bytes and cancellation/deadline checks; all HTTP completion, framing validation, byte limits and session identity checks execute in Rust. The original complete wire response follows the existing model, storage or gateway decoder and owned callback.

Activity model/snapshot/MCP transports and Gateway upstream I/O share Resolution.java for bounded Android DNS lookup. The original operation supplies its owner/deadline checks, while Rust retains destinations, messages, permissions and callback ownership. The service closes its resolver owner during teardown; completed late system lookups carry no application callback.

## Voice review state

Rust owns a distinct pending-review phase. The displayed operation changes to recognition or synthesis only when services admits its current owned dispatch. Losing or rejecting the held review pauses automatic capture; hiding the review preserves its proposal. The Resume microphone control uses the same current consent, pending-operation and draft checks as the actual capture-start operation.

Service interruption retires upload/response ownership, then clears and pauses only the matching voice request phase. This also covers late measured-memory admission refusals after request preparation. Text and current voice consent remain available. Another proposal, operation or task proactively pauses capture; a second admission check at utterance completion prevents that asynchronous callback from replacing separate work. Review and running-service ownership remain intact, and stale completed receipts cannot become recognition requests. Resume availability includes the same active-task condition as capture admission.

## Device authorization ownership

The optional Rust device grant shares the existing connection token transport, bounded sessions and credential adoption. Its private code stays inside the current flow map. Each network phase receives a unique nonce, so an old initialization or polling callback cannot authorize the next exchange. User status checks enforce the server interval and permanent slow-down increases. Current destination, workspace, ownership and expiration checks scope refusal to that grant. No report, test result, release catalog or planning artifact participates in application execution.

## Voice service lifetime

VoiceSession binds the current explicitly reviewed microphone session to a private Android microphone/playback foreground service. It uses a weak activity/controller reference, an immutable session-specific Stop intent and a bounded wake lease. Current notification/permission/owner/expiry checks stop only the affected session. Media starts no background AudioRecord before actual service admission. Capture start and cancellation share a critical section, preventing a late native start after cancellation returns. Activity pause suppresses UI presentation while the current background voice owner continues; activity destruction retires it. The process never restarts or replays a microphone session.

Background callbacks capture transport data only, not stale UI forms. The first activity return renders current core state before reading visible input again, preserving unsent core drafts. Pause-time input capture immediately reconciles native audio and service ownership. Ordinary previews use the existing complete teardown path; only owned synthesized audio survives background transition.

## Document provider ownership

The storage picker accepts installed Android document providers without package or account-type discovery. Rust retains the existing cloud access/state keys and legacy navigation action for saved-state compatibility, while the visible controls use Document providers. Local Android file authorities retain the phone access policy; other content authorities use the provider policy. An explicitly selected provider document also requires provider access when it is a local file.

Rust tracks up to64 selected document URIs and binds selection, text-read and grant-release callbacks to their current workspace and unique request. Opening a document externally creates no pending read owner. A document read cannot replace active primary work. Disconnect retires its own read while preserving primary callbacks, then requests release of only its recorded standalone read grants. Android preserves unrelated writable, tree and selected-phone grants. Confirmed cleanup removes the corresponding records; failed releases remain available for an explicit retry. Recovery retires an unfinished cleanup callback while retaining the grant records.

The Android binding checks current selection ownership and source access again on the UI thread before taking a returned read grant. Late or closed-activity returns acquire no persistent permission. If Rust does not adopt the selection, only a newly acquired read grant is released; an existing grant is retained. Android still authorizes every content access. No provider login is bundled or mandatory, and this picker does not claim application-owned cloud OAuth or SMB support.

## Pending snapshot certificate identity

Network storage credential selection is shared by manual requests and both payload implementations. The HTTP layer owns validated header formatting; model protocol selection retains its established default header names and OAuth path. Storage adds optional key/custom-header and destination-bound Bearer-token paths without introducing a dependency from the HTTP layer back to model or UI modules. Complete saved connection objects remain in the existing request/transfer ownership binding. Credential changes cannot certify an older response or retarget a pinned pending transfer.

The public-client authorization engine uses an explicit Model(index) or Storage owner, rather than creating a model profile to carry a storage grant. Each phase reads and updates only that owner's connection. Browser recovery persists its typed target while preserving old model-index records. Token callbacks retain process-owned nonces, current root/configuration binding and single-use adoption. Model refresh keeps its original model owner even when storage was the last authorization page. The HTTP layer checks a storage token's saved URL, trust identity, transport opt-in and authorization configuration against the current connection, plus actual expiry and uncertain-refresh state. Manual credentials are preserved and selected only by explicit mode choice. Authorization review discloses credential use and destination; storage setup retires any owned voice capture through the existing core/platform state reconciliation.

Voice preferences have a direct settings route separate from conversation admission. Rust owns numeric/toggle validation and stores preferences without requiring a model or speech peer. The native platform supplies its current permission observation and binds the explicit permission-settings effect to its own package. A preference save never emits a capture or network-start effect. Voice review still owns expiring consent and actual listener startup; settings navigation stops only its existing voice owner and keeps unrelated model/manual-proposal work.

Legacy inline/staged payloads and current streamed payloads apply the certificate DNS hostname from their pinned saved connection to every write, retry probe and verification request. The configured socket hostname/IP and port stay unchanged; the TLS/SNI hostname and HTTP authority use the reviewed certificate name. Android still validates the trusted certificate chain and hostname. Changes to the current storage settings cannot replace the pending transfer's pinned connection.

The shared identity helper recognizes text or base64 binary request framing, refuses ambiguous representations and finds authority only inside bounded complete request headers. It preserves all payload bytes, including non-UTF-8 values and text that resembles a Host header. Invalid certificate names, missing/duplicate authority and certificate aliases on HTTP refuse only the affected transport. No snapshot source, pending payload or existing archive format is rewritten by this repair.
