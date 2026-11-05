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

The microphone pauses during recognition, model work and playback to avoid recording the app's own speaker. During spoken playback, Speak now stops that owned playback and starts a new listener. It is an explicit button action; the app does not listen over its own speech. Stop playback pauses even if the native player has not started yet. Late playback completion, focus and dismissal events cannot replace the new microphone or another preview. This version supports automatic turns while the app is in the foreground; simultaneous interruption during speech and background listening are not implemented. A visible status shows Listening, Transcribing, Preparing reply, Preparing speech, Speaking or Paused. Stop voice ends the session. Leaving the foreground, changing conversation/workspace/routes or process restart also ends it. Consent expires after ten minutes and does not silently renew. Microphone/playback/service failure pauses the loop without automatic replay; Resume microphone starts another attempt while consent is still valid. Text remains available.

The review offers a silence interval of300–3000ms (default800), an activity threshold of100–5000 (default500; lower is more sensitive) and a5–30second utterance cap (default20). Capture uses16kHzmono16-bitPCM with a bounded pre-roll and requires approximately240ms of sustained activity. Quiet audio and short noise bursts are not uploaded. This is an adjustable energy detector, not a neural speech recognizer; room noise and microphone characteristics can require tuning. Ordinary operation reviews remain in force. A draft typed during a voice turn is preserved and a later transcript is appended for review rather than sent automatically.

Manual recording, standalone transcription and Speak last reply remain available. Manual recording stops at60seconds /2MiB. Microphone access requires platform permission; a pending permission request never starts an expired voice session. Capture originals are retained in the quota-managed local media folder and can be exported or individually reviewed for removal. Separate bounded media workers handle capture and file providers. Playback stops on app pause, loss of audio focus or conversation/workspace changes; owned callbacks reject retired or duplicate results.

Returned files appear in their conversation and can be previewed or exported. Configure a phone-folder or network destination in Files and storage before exporting; generated files retain their detected extension and content type. Supported native previews include PNG/JPEG images, platform-decodable audio/video, and PDF pages. A binary 3D scene can be retained/exported; an interactive 3D viewer is not implemented. Container identity and disk digest checks are distinct from actual decoder acceptance. Generic MP4 audio/video is classified from track handlers, not the filename.

Selected or returned PNG/JPEG images up to 2 MiB can be attached to a reviewed draft for any of the four model API formats; the chosen model must support image input. PDF OCR, document extraction and generation use whatever optional server routes you configure. PDF preview is available locally; built-in OCR, medical interpretation and a separate medical application are not part of this upgrade.

## Validation scope

`verification/services.py`, `media.py`, `jobs.py`, `voice.py` and `media_stream.py` exercise the public shared core with actual local HTTP peers, real file bytes, large-file streaming, review/cancellation/ownership, scoped fallbacks and all four primary/child model formats. Model responses and microphone callbacks in these checks are controlled fixtures. They do not certify an installed microphone, speaker, decoder or a particular remote server. Those require current-device and endpoint acceptance.

File transfers use single-use input results and expiring request ownership. A changed file, stale callback, cancellation or overflow cannot adopt a prefix as a complete input. Upload interruption after possible send never automatically replays the request. Missing optional servers do not block ordinary text or local work.
