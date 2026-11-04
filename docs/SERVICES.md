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

Direct POST binary results and JSON/base64 replies are bounded to 4 MiB. GET binary files and returned artifact URLs stream in 8 KiB chunks to owned files; large video bytes do not enter conversation JSON. Native input selection is limited to 2 MiB. Local media has a 512-file / 256 MiB total bound. Interrupted partial files remain visible in Manage local media files, where each file can be exported or individually reviewed for permanent removal.

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

Conversation actions → Voice and media offers explicit recording, standalone transcription and Speak last reply. Enable voice conversation first to review recognition, speech and model destinations. Each deliberate Record / Finish turn can then transcribe, submit the transcript through the primary conversation and play the returned audio. Consent expires after ten minutes, process restart, relevant settings changes or Stop voice. A pre-existing composer draft is preserved and the transcript is appended for review. Ordinary operation reviews remain in force. No open microphone is enabled.

Audio recording uses the platform microphone permission, is foreground only, and stops at 60 seconds / 2 MiB. Capture originals are retained in the quota-managed local media folder, including interrupted recordings; they can be exported or individually removed there. Capture and file-provider workers are separate from ordinary network workers and have bounded queues and cancellation. Playback stops on pause, loss of audio focus or conversation/workspace changes. Core ownership checks reject stale or duplicate callbacks.

Returned files appear in their conversation and can be previewed or exported. Configure a phone-folder or network destination in Files and storage before exporting; generated files retain their detected extension and content type. Supported native previews include PNG/JPEG images, platform-decodable audio/video, and PDF pages. A binary 3D scene can be retained/exported; an interactive 3D viewer is not implemented. Container identity and disk digest checks are distinct from actual decoder acceptance. Generic MP4 audio/video is classified from track handlers, not the filename.

Selected or returned PNG/JPEG images up to 2 MiB can be attached to a reviewed draft for any of the four model API formats; the chosen model must support image input. PDF OCR, document extraction and generation use whatever optional server routes you configure. PDF preview is available locally; built-in OCR, medical interpretation and a separate medical application are not part of this upgrade.

## Validation scope

`verification/services.py`, `media.py`, `jobs.py`, `voice.py` and `media_stream.py` exercise the public shared core with actual local HTTP peers, real file bytes, large-file streaming, review/cancellation/ownership, scoped fallbacks and all four primary/child model formats. Model responses and microphone callbacks in these checks are controlled fixtures. They do not certify an installed microphone, speaker, decoder or a particular remote server. Those require current-device and endpoint acceptance.
