## Native local HTML capture

The native `capture` operation reads the selected workspace HTML file and selected workspace TrueType font, or a compatible Android system font when the font field is blank. Font data is read through original Rust code; no external analysis/browser program is installed or launched. It creates a new PNG in the app workspace and verifies the bytes. It never overwrites an existing filename. Captured pixels can contain the selected document's visible data; the result identifies the output file and observed rendering scope. Existing operation review and agent tool choices apply. The PNG is not automatically uploaded or inserted as a model image; the textual operation result follows the current model route. Export the file explicitly through the existing project-file storage action. Interrupted new output can remain for inspection.

The current renderer handles supported local HTML/CSS and unhinted TrueType glyphs only. It does not fetch external resources, run scripts, retain browser cookies or create a browser session. Complete web capture/interaction remains active work. Android default-font resolution has compiled bindings but still needs installed-device validation. No earlier permission or storage control is removed.


## Local model URL entry — HTTP default correction

When no scheme is entered, model setup uses HTTP for private, loopback and link-local IPs, the 100.64.0.0/10 shared address range used by Tailscale, localhost/.localhost/.local names, and addresses entered with the Local API format. An explicitly entered http:// or https:// scheme wins. Public chat-provider hostnames still default to HTTPS. The saved URL shows the selected scheme; there is no automatic TLS-error fallback or request replay. Existing saved HTTPS URLs are not silently rewritten: edit an incorrectly saved local endpoint and enter http:// explicitly or re-enter its bare local IP.

HTTP is not application-layer TLS. Use the scheme your server actually supports and a network you trust. Selecting a local API format does not prove that a remote host is private or secure.

# Access, investigation features and your data

RLLM deliberately supports user-controlled file inspection and reviewed tools. These capabilities are features. Their existence does not make unauthorized access, unintended disclosure, unsafe writes or an incorrect permission check acceptable: those are defects to report. Enable the access you want, and disable capabilities you do not want. Protect your data to the level you need.

## Implemented controls

| Capability | What it can access or send | Default and how to limit or disable it |
| --- | --- | --- |
| Phone file browsing | Lists and reads UTF-8 files within primary shared storage exposed by Android; reads stay within the existing 1 MiB bound and listings within 500 entries. It does not automatically scan the phone. | Broad access is requested once on first startup of this version. Android requires your explicit grant. App settings → Files and access → Limit browsing to selected folder or Disable phone file tools. Android settings can separately revoke All files access. A denial is respected and does not cause repeated automatic prompts. |
| Selected-folder phone tools | Read/list the selected Android folder; create new files there or update existing text through the separate revision workflow. SD cards and document providers use this folder picker. | Requires an explicit Android folder grant. Disable phone file tools blocks these tools. Existing folder grants are retained so you can enable them again. |
| Import a document into the composer | Reads the single document you explicitly select and puts its text in the draft. | Only runs when requested. Do not select a document or send the resulting draft if you do not want it used. This explicit import is separate from phone tool scope. |
| Model requests and reviewed agent tool results | The selected model endpoint receives messages and tool evidence included in a request. A remote endpoint receives that data outside the phone. Main and delegated agents can have different configured endpoints. | You choose the connection and approve tool operations. App settings → Resources → Pause remote model requests stops model requests while keeping file tools available. Reject an operation to prevent its execution. Broad Android permission does not itself approve an agent tool operation. |
| Default storage and reply autosave | Saves to the configured WebDAV destination or explicitly selected phone folder. Broad browsing does not silently change this destination. | App settings → Storage controls the destination and Save each completed reply. Turn autosave off to stop automatic reply saves. Manual saves remain explicit actions. Disabling phone file tools does not silently alter an already configured save destination. |
| Phone API gateway | Serves authorized API clients while explicitly started, using its configured bearer token and listen address. LAN binding exposes it beyond loopback. | App settings → Network services → Phone API gateway → Stop gateway. Avoid LAN binding or unencrypted LAN traffic unless you intend that exposure. |
| Web and configured MCP tools | Sends reviewed requests to the configured destination; returned content is untrusted and can become model evidence. | For a persistent agent restriction, Agent settings → Tool access → Edit selected agent: remove web_read, web_search or rea from its permitted tools. Explicit manual actions remain available. Do not configure or approve tools you do not want used. Review the destination and arguments, and reject unwanted operations. A tool response never grants itself permission. |
| Local script execution | Runs submitted source in an Android isolated process without RLLM's file or network permissions. | Requires review; reject the operation to prevent it. Remove script_check and script_run from the selected agent's permitted tools to disable its script proposals. Explicit manual actions remain separate. This does not give scripts the broad phone grant. |

“Disable phone file tools” disables phone_list, phone_read, phone_create and phone_update tools. The explicit document picker, export and configured save workflows have their own controls as described above. Limited mode remains authoritative for browsing even if Android still grants broad access. Scope changes while an operation is busy are refused; reviewed reads cannot silently change scope at approval.

## Android access boundaries

All files access is Android's name for broad shared-storage access. It is not root access and does not open another app's private sandbox, all protected system paths, or every document provider. This version browses primary shared storage directly; use the selected-folder option for SD cards and other providers. Android and provider restrictions still apply. The permission screen cannot lift those restrictions.

App settings → App inspection now provides an explicit current-user app catalog and selected-package observations. The QUERY_ALL_PACKAGES manifest declaration permits broader package visibility; Android does not show a runtime grant dialog for it. Collection begins only when you request the catalog. Select a package, then choose Read / refresh selected app details to read its version, UID, system-app flag and requested permissions with PackageManager grant status. Catalog labels are app-supplied and may be abbreviated; the exact package is always shown. A missing/mismatched permission flag array is reported as unknown, not denied. These are platform-reported declarations and grants, not observed use or proof of effective special access/AppOps permission.

Before reading details or usage you can choose Exclude this app's observations, including for banking or other sensitive apps. Exclusions are exact package identities and persist across restart. They stop selected-package details and usage collection; basic catalog labels/package identities remain visible so you can manage exclusions. A shared UID does not make package exclusions UID-wide isolation. Disable inspection and clear observations stops the feature, cancels an in-flight app observation and clears its current catalog/details from saved application state while retaining exclusions. This is not a secure-erasure guarantee for earlier backups. Package queries use one dedicated worker with no waiting-job queue. If Android blocks a package query, cancellation invalidates its results; it cannot forcibly interrupt Android Binder. Further app queries refuse while that worker remains occupied, while file persistence and model/network workers remain independent. Disabling inspection does not revoke independent file permissions, affect other software, or change the explicit document/save controls above. Enabling inspection performs no collection until requested. Current observations and the catalog are encrypted with the existing application state and are never automatically added to model messages or tool evidence.

Selected-app usage observations are implemented in the current development source as described below; runtime performance and network observations remain roadmap work. This slice does not read private banking data, decrypt another app's files, monitor traffic or determine spying from a permission list. It covers the current Android user/profile exposed by PackageManager, not every device profile. Future additional privileges and collection need disclosure and effective disable controls before delivery.


## Selected-app usage observations

In **App settings → App inspection**, usage observations have a separate enable control, off by default. Grant usage access through Android settings, then explicitly request the selected app's last 24 hours or last 7 days. Returning from Android settings does not start collection. This feature requires Android 15 or later because its system query filters to the exact selected package; older versions retain package/permission inspection without collecting all apps' usage as a fallback.

The display contains four Android-reported counts: activity resumes, activity pauses, foreground-service starts and foreground-service stops. These are lifecycle records, not app launches, elapsed runtime, CPU use, network traffic or a spyware verdict. Android retains events for a limited period: a seven-day query does not promise seven complete days of history. An empty successful result means no matching retained records were returned, not proof that the app was unused or safe.

Disable usage observations to clear their display while keeping package metadata available. The master inspection switch and exact-package exclusions also apply. A retained Android usage-access grant does not override these RLLM controls; revoke that OS grant separately in Android settings if desired. Changing selection, disabling, excluding or cancelling invalidates pending results. Android's system query cannot be forcibly cancelled once underway; RLLM discards obsolete results.

RLLM saves the latest four counters, selected package and observation/query timestamps in encrypted application state rather than retaining an event log, with a 10,000-event and two-second processing bound. Exceeding either refuses the observation instead of presenting partial counts as complete. The Android service can allocate its query result before RLLM iterates it. Locked user storage, missing/revoked grants and unavailable results are errors, not zero usage. Observations are not automatically sent to models or adopted as tool evidence.

Current status: independent scoped source, C ABI and Android compatibility checks pass; no installed-device or OEM acceptance is claimed. Official API references: [selected-package query](https://developer.android.com/reference/android/app/usage/UsageEventsQuery.Builder#setPackageNames(java.lang.String...)) and [usage events](https://developer.android.com/reference/android/app/usage/UsageStatsManager).

## LLM trust and data sharing

Do not use a model package, inference runtime or service you believe may be compromised. Downloaded model weights and the software that loads them are different components; a modified or untrusted runtime can put accessible data at risk. Running locally is not a security guarantee. RLLM cannot certify your selected model, runtime, plugins or server.

You may choose a local or remote LLM. Your information remains yours, but a selected service or compromised local software may retain or disclose what it receives or can access. RLLM cannot promise that “everything stays in this app,” revoke copies already sent to a service, or protect data from other software with independent privileges. Inspect connection addresses and tool arguments, restrict access, avoid sending sensitive files, and use the disable controls appropriate to your needs.

## Validation limits

Permission declarations and connected host tests do not prove the behavior of every Android/OEM permission screen or document provider. Installed Samsung/device acceptance remains necessary. This is a disclosed test build, not full-project or production acceptance. The separately recorded archive independent-review restriction remains unresolved and is not replaced by file-permission checks.

Android primary references: [All files access](https://developer.android.com/training/data-storage/manage-all-files) and [Storage Access Framework](https://developer.android.com/training/data-storage/shared/documents-files).

## Web access and blocked operations

`web_read` fetches a known HTTP/HTTPS address and extracts static text; it does not run website JavaScript or control a browser. `web_search` additionally needs a configured search endpoint. Agent tool permissions and the explicit review step apply to both. The current development source records blocked, rejected, interrupted and failed web operations in conversation context so later model replies can distinguish an actual failure from a result that never arrived. It does not automatically retry, enable a disabled tool or turn prose containing JSON into an executed call.

Headless browser sessions, visual click/type/scroll actions and challenge-verification handoff are requested roadmap work. They are not implemented by the static reader or by the failure-feedback repair. A browser engine and its deployment location must be selected before those capabilities can be claimed. See verification/browser-plan.md for the completion contract.

## Model response formatting

The harness accepts reviewed tool proposals as bare JSON. A model response beginning with reserved `<tool_call>` or `<tool_calls>` markup is an unsupported tool format; RLLM does not extract or execute embedded JSON from it. New such replies are retained as raw failed responses in encrypted history with a clear message in the conversation. Tap the message to inspect the raw text. Regenerate reply in new branch is an explicit new model request, not automatic tool replay. Existing historical assistant markup is similarly identified in presentation without changing its raw content. Inline, quoted and fenced examples remain ordinary text. Correct bare-JSON proposals still require existing permission checks and review.


## Native investigation and local comparison

Investigation is part of ordinary and recursive tasks. The local Rust `inspect` operation reads the selected workspace file and reports observed bytes, document structure and supported format fields. File sampling and each analyzer state their coverage; a sampled prefix is not treated as the whole file. `compare` reads two selected workspace files and reports positional byte differences; matching incomplete prefixes do not establish full-file equality. Neither operation launches Node, REA, desktop analyzers or an external analysis server.

The model can propose these internal operations as part of an ordinary question; the existing operation review and agent tool choices remain. Results return through the originating task/model route, so approved observations may be sent to that configured model provider. Additional investigation guidance preferences do not switch off the underlying native process. An optional configured remote MCP connection remains separate from local analysis. Full universal investigation and rendered webpage capture are still implementation work.


The native Rust-source and ELF passes read only the already captured inspection bytes. They perform no network requests, program execution, dependency loading or additional file reads. The source pass reports lexical names/locations; the ELF pass reports declared offsets, mappings and names. Existing file access and task routing remain in force.


HTML structure and CSS token summaries are produced locally from captured bytes. They contain counts/status; the internal trees preserve source spans for future native rendering work. The parsers do not fetch referenced URLs, execute scripts or render pages. Existing extraction/file samples remain separate returned evidence.

## Selected chat attachments

The Android file picker reads only the file you select for attachment. PNG/JPEG image bytes are retained with the conversation draft in encrypted app state and sent to the selected provider only when you send or explicitly start a task using that draft. UTF-8 text files append to the draft. Switching chats, changing the agent/connection or restarting invalidates pending picker results. Removing an unsent image removes it from that draft; previously sent images remain in conversation history and provider requests until context is explicitly changed. The UI receives attachment metadata without a duplicate encoded payload.
