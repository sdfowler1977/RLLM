# Optional services

This is the implementation contract for the next upgrade. The tagged recovery baseline does not yet implement these new service routes. Existing local operations, model connections, web search, investigation, reviewed operations and child-to-parent return remain the starting behavior.

## Configuration

Use versioned, provider-neutral service connections independent of language-model profiles. A connection contains a user-defined name, host URL, transport options, optional authentication header/prefix or other user-controlled headers, enabled state, timeout and resource limits. Credentials stay in the existing encrypted settings; catalogs, diagnostics, exports and retained results must redact them.

A route identifies a capability, connection, method and relative path, request format/template, input mapping, response format/mapping, and optional job/status/cancellation paths. Support local, LAN, reverse-proxy and internet hosts. Do not infer service capabilities from a model's provider name or reuse a coding endpoint merely because its key matches. The configured intermediate server may expose an entirely different protocol and capability set.

Defaults choose an ordered list of eligible routes per capability, with explicit fallback conditions. Different capabilities may share one host or use separate hosts. A connection or route can be disabled without losing its settings. Nothing is probed at startup unless the user explicitly requests a connection check.

## Runtime

The shared Rust harness resolves a capability to its current configured route, validates inputs and permission, displays destinations and disclosure scope for approval, then issues the existing owned network operation. Manual, primary-model, recursive and delegated requests use the same dispatch and result path. A child result returns to its captured requesting child and parent, independent of the currently visible chat or model selection.

Use bounded, expiring request ownership, single-use callbacks and current-operation cancellation. Configuration, permission, workspace or requesting-route changes invalidate old proposals and callbacks. An interrupted submission is not automatically replayed. A transport error after request bytes were sent is an uncertain submission, not permission to start another billed generation. Only explicit configured conditions may advance to another fallback, once per route. Authentication/input failures do not silently cascade to other destinations.

An accepted asynchronous job is not a completed artifact. Retain its actual server job identity and state, reconcile completion through the configured status route, validate the returned artifact and preserve its provenance. Follow artifact URLs only within the user-approved scope; never forward credentials automatically to another origin. Failed playback, transcription or generation leaves the text conversation and unrelated capabilities available.

| Capability | Input | Result and integration |
| --- | --- | --- |
| Speech recognition | Explicit microphone capture or selected audio | Transcription returned to the draft for review or the active approved voice turn |
| Speech synthesis | Selected text or an assistant response | Actual returned audio with play, stop and cancellation |
| Image generation | Prompt, parameters and optional selected images | Validated image artifact shown in the existing conversation |
| Video generation | Prompt, parameters and optional selected media | Tracked job and validated playable video artifact |
| Scene generation | Prompt and optional selected source material | Typed scene/model artifact with supported preview and export |
| Document extraction | Explicitly selected document/pages | Extracted text and page provenance; optional OCR route for scanned pages |

Request/response formats are adapter capabilities, not provider labels. Implement typed mappings for JSON/text first, then binary/multipart media and asynchronous jobs. Treat service descriptions, OCR, transcriptions and generated results as untrusted data; they never grant tool permissions or become harness instructions. Preserve exact reported failures instead of manufacturing empty successful output.

## Rollout

1. Add service connection/route settings, safe request/response mappings, explicit primary/fallback configuration, reviewed manual execution and shared-model execution. Verify real HTTP peers, isolated failures, permission changes and callback ownership.
2. Add bounded artifact storage and transfer outside the encrypted conversation JSON, media validation, job polling/cancellation and resumable job reconciliation. Verify restart and uncertain-submission behavior.
3. Add push-to-talk capture, transcription, text reply and speech playback as one conversation turn. Keep each operation independently usable and optional.
4. Add image, video and scene presentation/upload paths through the same artifact and job layer. Add document/PDF text extraction and optional OCR before the later domain-specific branch.
5. Add explicit open-microphone opt-in only after ordinary speech works, with a visible capture state, reliable stop, turn boundaries, interruption handling and echo prevention. No background microphone permission or capture is implied by enabling another capability.

Each layer is implemented and validated in the same app before the planned product split. One configured model must continue working if another model or any optional service is absent, malformed, disabled or offline. Model-free workspace functions continue without AI. Missing optional service settings, tests, plans or reports never become startup or ordinary-runtime gates.

## Validation boundaries

Exercise ordinary and child requests in all four supported provider formats, exact target/fallback selection, keyless and authenticated peers, different host credentials, scoped HTTP/parser/timeouts, uncertain writes, cancellation, duplicate/late callbacks, configuration changes during review, unsupported secondary models and artifact-free local operations. Validate Android microphone, audio focus, playback, lifecycle, storage and permission behavior on the actual installed build before claiming device acceptance. Owner checks do not substitute for independent acceptance.
