## Question process

The current complete source contains137 build inputs/96 Rust modules. `core/inquiry.rs` is owned by the existing engine; application dispatch, the existing hypotheses operation and task/provider lifecycle use it directly. All four existing provider formats share process guidance and owned tool completions. Without a model, use Investigation → Question and design process. No added runtime dependency. See [process guide](QUESTION-PROCESS.md), [current scope](STATUS.md) and [release notes](../release/NOTES.md). Earlier instructions below remain preserved; latest release facts govern this version.

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

## Platform boundaries

The Rust core offers reusable domain logic, not an already portable end-user application. Android services, Keystore, document URIs, UI lifecycle and platform network bindings need replacements on another operating system. The archive implementation includes Linux/Android-specific descriptor bindings. See [the iOS assessment](IOS.md).

## Native investigation execution

`core/exploration.rs`, `core/comparison.rs` and `core/binary_observation.rs` implement original Rust structural document observations, held-file byte comparison and selected binary envelopes. `investigation.rs` connects these readers to actual file input. Shared application dispatch and harness result paths carry observations into ordinary chat and recursive tasks. Investigation preferences add task guidance, and configured remote MCP connections provide tool catalogs and reviewed calls. [Question and design process](QUESTION-PROCESS.md).

## Response framing

network.rs owns incremental HTTP response framing and bounded, owned response sessions. transfer.rs reuses it for verified snapshot streams. The Wire.java platform binding supplies Android InputStream bytes and cancellation/deadline checks; all HTTP completion, framing validation, byte limits and session identity checks execute in Rust. The original complete wire response follows the existing model, storage or gateway decoder and owned callback.

Activity model/snapshot/MCP transports and Gateway upstream I/O share Resolution.java for bounded Android DNS lookup. The original operation supplies its owner/deadline checks, while Rust retains destinations, messages, permissions and callback ownership. The service closes its resolver owner during teardown; completed late system lookups carry no application callback.
