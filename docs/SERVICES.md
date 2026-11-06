# Optional server capabilities

Use App settings → Network services → Service connections. These connections are independent of LLM connections. The app still opens and supports local work with no model; conversation inference needs one configured model. Speech, generation, document services and additional models are optional. A disabled, malformed or offline unrelated service does not block an eligible service or the primary model.

Configure the HTTP interface exposed by your own server, local network, tunnel, internet service or proxy. There are no provider-specific key slots. A server can dispatch to any program behind that interface. The phone does not install or start server programs. Services using a different protocol need an HTTP adapter on the server.

## Connection format

Save one version-1 JSON object with `hosts`, `routes` and `defaults`. Saving never sends a request. Host URLs contain the scheme, hostname and optional port; route paths contain the API path and query. Explicit `plain:true` is required for HTTP. HTTPS retains certificate and hostname validation.

```json
{
  "version": 1,
  "hosts": [
    {"id":"local","url":"http://server.local:8080","plain":true,"enabled":true},
    {"id":"backup","url":"https://speech.example.invalid","enabled":true,
     "auth_header":"Authorization","auth_prefix":"Bearer ","key":""}
  ],
  "routes": [
    {"id":"recognize","host":"local","capability":"speech_recognition",
     "method":"POST","path":"/recognize","enabled":true,
     "request_format":"multipart","file_field":"audio","request":{},
     "response":"json","response_path":"/text"},
    {"id":"speak-local","host":"local","capability":"speech_synthesis",
     "method":"POST","path":"/speak","enabled":true,
     "request":{"text":{"$input":"/text"}},
     "response":"binary","artifact_kind":"audio"},
    {"id":"speak-backup","host":"backup","capability":"speech_synthesis",
     "method":"POST","path":"/audio","enabled":true,
     "request":{"input":{"$input":"/text"}},
     "response":"base64","response_path":"/audio","artifact_kind":"audio"}
  ],
  "defaults": {
    "speech_recognition":{"routes":["recognize"]},
    "speech_synthesis":{"routes":["speak-local","speak-backup"],
                        "fallback_codes":[503],"connect_fallback":true}
  }
}
```

The example URLs and paths are illustrative; replace them with your server contract. No key is needed for an unauthenticated local service. Hosts accept an optional `headers` object, any valid `auth_header` and an optional prefix. Each host's credentials stay with its own request. Duplicate headers differing only by letter case are refused. A custom multipart Content-Type must not replace the generated boundary.

Saved keys and header values appear as `<unchanged>` in the editor. Keep that marker to retain a saved value, replace it to change the value, or use an empty key to remove it. Changing the host URL or authentication settings requires re-entering masked credentials. This prevents a configuration edit from silently forwarding an old key to a new host.

## Requests and responses

Route IDs, host IDs and capability names use letters, digits, underscores or hyphens. Capabilities can include `image_generation`, `video_generation`, `scene_generation`, `document_extraction` or other names you choose. Speech conversation uses `speech_recognition` and `speech_synthesis` defaults.

| Setting | Behavior |
| --- | --- |
| `method` | GET, POST, PUT or DELETE |
| `path` | Relative HTTP path; `{field}` substitutes a scalar from input with URL encoding |
| `request` | JSON template; `{"$input":"/prompt"}` copies the actual typed input value; other values remain literals |
| `request_format` | `json` (default), `binary` selected file, or `multipart` mapped fields plus selected file |
| `$file` | A single-key mapping with `name`, `mime`, `base64` or `data_url`; requires explicit file upload consent |
| `file_field` | Multipart file field name; defaults to `file` |
| `input_schema` | Optional JSON schema object (8 KiB maximum) describing the input expected by your server |
| `response` | `json`, UTF-8 `text`, `binary`, `base64`, or `artifact_url` |
| `response_path` | JSON pointer selecting the result, encoded bytes or artifact URL; empty selects the whole document |
| `artifact_kind` | `audio`, `video`, `image`, `document`, `scene` or generic `data`; container mismatch is refused |
| `artifact_host` | Host allowed to serve a returned artifact URL; defaults to the submission host |
| `artifact_limit_mb` | Streaming file limit, default 64 MiB, maximum 256 MiB |
| `timeout_seconds` | Request deadline, up to 600 seconds |

JSON pointer escapes follow `~0` for tilde and `~1` for slash. A whole-input mapping uses `{"$input":""}`. Path substitutions currently select top-level input fields. Response pointers are fixed pointers; use a server adapter for APIs whose response keys or protocol need additional normalization.

The requesting model sees eligible route IDs, capabilities, input paths, optional input schemas and response formats. Host credentials and literal request-template values are excluded from that catalog. It can call the normal reviewed `service` operation from ordinary or delegated work. Explicit manual requests use the same engine. A capability request follows its configured ordered routes; an exact route request stays on that route. Review shows the destinations and explicit file disclosure.

Only configured HTTP error statuses or a confirmed failure before send can select a fallback. A sent request with an uncertain outcome is never automatically replayed. Failed artifact downloads do not repeat generation. Returned URLs must match the configured artifact host's scheme, hostname and port; redirects and credential forwarding to another origin are refused.

JSON/base64 replies are bounded to 4 MiB. Direct binary POST/PUT results, GET binary files and returned artifact URLs stream in 8 KiB chunks to owned files, using artifact_limit_mb (default64MiB, maximum256MiB within available space); large video bytes do not enter conversation JSON. Initial submissions accept complete200or201file responses. Retrieval requires200; partial206responses and redirects are not treated as complete artifacts. Other initial status bodies are bounded to64KiB and drained for complete framing before configured HTTP-error fallback can run.202without a usable artifact remains accepted_unconfirmed and never resubmits generation. Confirmed pre-send fallback releases its previous reservation first. Expired abandoned transfers release unwritten reservations at the next stream admission or expired read. Selected files import in 8 KiB chunks up to 64 MiB, within remaining storage; file bytes stay outside conversation JSON. Reviewed uploads stream binary, multipart and configured JSON base64/data-URL mappings. Inline legacy capture remains limited to 2 MiB; ordinary model image attachments retain their separate 2 MiB limit. GET routes are lookups and cannot upload an explicitly selected file; use a POST or PUT route for uploads. Local media has a 512-file / 256 MiB total bound. Stream allocations reserve their maximum logical size before writing, so concurrent imports and downloads cannot each spend the same space. Manual recording reserves its current capture bound while the platform writes. Cancellation releases unwritten padding and retains received bytes; an abrupt process loss can retain reserved padding in an unadopted partial file. Interrupted partial files remain visible in Manage local media files, where each file can be exported or individually reviewed for permanent removal.

## Generation jobs

A submission route can have this optional `job` mapping. The submission must return JSON; status, result and cancellation are separate routes.

```json
{
  "id":"/id","state":"/state","success":["done"],"failure":["failed"],
  "accept_codes":[200,202],
  "status_route":"status","result_route":"result","cancel_route":"cancel",
  "result_input":{"asset":{"$input":"/output/file"}}
}
```

Status and result routes can use `/jobs/{job}` or other configured paths. The captured server identifier becomes input `job`. Optional `status_input` and `cancel_input` templates map from the submission response. `result_input` maps from a successful status response and can supply other result path/request fields. This mapping uses the same `$input` syntax and preserves scalar types. Terminal status values may be distinct strings, numbers or booleans. Default acceptance is HTTP202; HTTP200 acceptance must be explicitly configured.

Server acceptance is tracked separately from completion. A successful status becomes `ready`; the job becomes `completed` only when a validated local artifact arrives. Refresh and retrieval never repeat submission. App restart preserves job records, but does not start an unattended polling timer. Cancellation is a reviewed server request; an accepted cancellation does not prove the server stopped. Job records can be archived, restored or individually reviewed for local removal. Removing a record does not cancel a server job or delete its files.

## Voice, files and documents

Conversation actions → Voice and media → Start open microphone opens a review of recognition, speech and model destinations. Enable the listening checkbox and confirm to start. The app captures microphone PCM directly, detects an utterance locally, stops capture, sends the actual WAVE recording to your recognition connection, sends its transcript through the primary conversation, plays the returned speech, then starts the next listener only after playback completes. It does not depend on keyboard dictation or the standard platform speech recognizer.

Both `speech_recognition` and `speech_synthesis` defaults are required for this mode, alongside one conversation model. A synthesis server/key alone provides speech output, not recognition. Configure the actual paths, request fields, response mapping, host credentials and fallback order exposed by your servers. Recognition must accept actual recording bytes through binary, multipart or a `$file` base64/data_url mapping and return text or JSON selecting text. Speech output must return audio bytes, base64 audio or an approved artifact URL. No provider-specific settings or mandatory paid key are required.

The microphone pauses during recognition and model work. By default it also pauses during playback. The optional “Listen for interruption during replies” review setting permits microphone capture while the reply plays, only while the app is in the foreground and device echo control is active. Approximately240ms of sustained microphone activity stops that owned reply and promotes the same PCM capture into the next utterance, preserving its leading speech. Echo control availability and actual suppression vary by device and audio route; installed-device effectiveness remains unverified. If echo control is missing or stops, only the interruption listener is retired and the reply continues. Speak now remains available to stop the owned reply and start a new listener. Stop playback pauses even if the native player has not started yet. Late completion, focus, dismissal and interruption events cannot replace another capture, primary request or preview. A typed draft stops the interruption listener and remains available for review. The app does not take over an existing call's communication mode. Background listening is not implemented. Visible status distinguishes the starting interruption microphone from an active interruption microphone, alongside Listening, Transcribing, Preparing reply, Preparing speech, Speaking and Paused. Stop voice ends the session. Leaving the foreground, changing conversation/workspace/routes or process restart also ends it. Consent expires after ten minutes and does not silently renew. Microphone/playback/service failure pauses the loop without automatic replay; Resume microphone starts another attempt while consent is still valid. Text remains available.

The review offers a silence interval of300–3000ms (default800), an activity threshold of100–5000 (default500; lower is more sensitive) and a5–30second utterance cap (default20). Capture uses16kHzmono16-bitPCM with a bounded pre-roll and requires approximately240ms of sustained activity. Quiet audio and short noise bursts are not uploaded. This is an adjustable energy detector, not a neural speech recognizer; room noise and microphone characteristics can require tuning. Ordinary operation reviews remain in force. A draft typed during a voice turn is preserved and a later transcript is appended for review rather than sent automatically.

Manual recording, standalone transcription and Speak last reply remain available. Manual recording stops at60seconds /2MiB. Microphone access requires platform permission; a pending permission request never starts an expired voice session. Capture originals are retained in the quota-managed local media folder and can be exported or individually reviewed for removal. Separate bounded media workers handle capture and file providers. Playback stops on app pause, loss of audio focus or conversation/workspace changes; owned callbacks reject retired or duplicate results.

Returned files appear in their conversation and can be previewed or exported. Configure a phone-folder or network destination in Files and storage before exporting; generated files retain their detected extension and content type. Supported native previews include PNG/JPEG images, platform-decodable audio/video, and PDF pages. Returned GLB2scene triangle geometry can now be previewed inside the app. Drag to orbit, or use the labeled rotate, tilt, zoom and reset buttons. The Rust software viewport applies node transforms, sparse/interleaved accessors, supported unsigned indices, triangle/strip/fan topology, base/vertex colors and depth testing. The preview explicitly omits textures, animation and physical material effects; it does not fetch external resources. Skins, morph targets, transparent materials, unsupported required extensions, invalid geometry and excessive preview workloads refuse only preview; the original file remains available for export. Each viewport is bound to the returned file, workspace and conversation; source changes, context changes, close and foreground loss retire its use. Measured heap/system memory and the app resource mode admit metadata complexity before JSON node allocation, then scale geometry, accessor spans and viewport resolution within that budget. Missing measurements use a conservative usable default. Absolute ceilings of two in-flight/live viewports,100000triangles,300000accessor elements,16MiB accessor spans,4MiB scene JSON,128–768pixel frames and bounded frame work/time protect memory and UI responsiveness. Native controls use the admitted frame size. Supported larger original files retain the existing256MiB media limit. Installed-device controls, gesture/lifecycle behavior and display quality remain unverified. GPU rendering and broader scene support remain open. Container identity and disk digest checks are distinct from actual decoder acceptance. Generic MP4 audio/video is classified from track handlers, not the filename.

Selected or returned PNG/JPEG images up to 2 MiB can be attached to a reviewed draft for any of the four model API formats; the chosen model must support image input. PDF OCR, document extraction and generation use whatever optional server routes you configure. PDF preview is available locally; built-in OCR, medical interpretation and a separate medical application are not part of this upgrade.

## Validation scope

`verification/services.py`, `media.py`, `jobs.py`, `voice.py`, `voice_interruption.py` and `media_stream.py` exercise the public shared core with actual local HTTP peers, real file bytes, large-file streaming, review/cancellation/ownership, scoped fallbacks and all four primary/child model formats. Model responses and microphone callbacks in these checks are controlled fixtures. They do not certify an installed microphone, speaker, decoder or a particular remote server. Those require current-device and endpoint acceptance.

File transfers use single-use input results and expiring request ownership. A changed file, stale callback, cancellation or overflow cannot adopt a prefix as a complete input. Upload interruption after possible send never automatically replays the request. Missing optional servers do not block ordinary text or local work.

Android capture uses the microphone session’s platform echo-cancel effect; availability, enabled state and communication mode are checked before advertising an interruption microphone. Interface reference: https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler . A positive platform state does not certify physical echo removal.

The static scene reader follows the GLB2container and accessor/node conventions in https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html . This is a geometry preview subset, not a claim of full rendering conformance. `verification/scene.py` executes actual returned files, transforms, sparse/interleaved data, depth-tested pixels, camera changes, source mutations and scoped refusals.

The static scene viewport now divides rasterization into disjoint row bands with a shared bounded host CPU allowance. It preserves triangle draw order and exact pixels, accounts for worker stack memory, and uses one worker when current memory or measured CPU limits require it. Closing a queued or active frame releases admission before returning; no partial frame is reported as successful.

Headphone/Bluetooth output disconnection and speech audio-focus loss stop current foreground audio. Speech also pauses when another app asks it to duck. The owned voice session is retired, so neither a delayed recognition/synthesis result nor focus recovery automatically opens the microphone. An already-running primary model answer can still finish into the conversation. Start voice again through its review when the audio route is ready. Route monitoring is scoped to active audio/voice and the foreground lifecycle; unsupported route/focus access fails only the optional audio operation. Physical phone, headset, Bluetooth, call and lifecycle behavior still needs device acceptance.

Voice request review now has an explicit waiting state. Dismissing or going back from review keeps the unsent proposal available. Rejection pauses voice and releases its capture owner; Resume microphone starts a fresh listener. Approving a request changes the status only after the service operation is admitted. An explicitly requested transcription returns to the composer for review and never sends itself to the model.

Resume microphone is enabled only when the same Rust checks used to start capture allow it: the reviewed session is live, no operation or proposal is pending, and the composer is empty. A retained transcript can be edited or sent without opening the microphone; clearing it enables explicit resume.

If current memory admission refuses a recognition or speech request, its owned transfer is retired and the voice loop shows Paused. The abandoned voice phase is cleared, the primary text answer remains available, and nothing is submitted or automatically replayed. Explicit resume opens a fresh microphone while the reviewed session remains valid. Service cleanup changes only its matching voice phase.

Starting a separate request review, operation or task pauses the current microphone. A completed utterance arriving afterward cannot replace that proposal or running request, submit recognition or change the conversation. The separate request can still be approved and completed normally. Resume microphone stays unavailable while work is pending and starts a fresh capture only when explicitly chosen afterward.

If an otherwise completed answer cannot enter speech preparation, including the speech-text size bound or full local media storage, the loop also shows Paused with its specific explanation. The complete primary text remains in the conversation. It is not truncated for speech or automatically sent again.
