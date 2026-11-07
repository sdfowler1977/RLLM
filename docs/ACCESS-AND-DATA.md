# Files, services and source access

Choose source access in Workspace → Setup → Access permissions. Configure phone folders and browsing scope in Workspace → Setup → Phone folders and scope. Choose each agent's permitted operations in Agent settings → Tool access. Review shows the operation, arguments and destination before execution.

| Action | Source and settings |
| --- | --- |
| Browse phone files | Primary shared storage through Android's All files access grant. Select Limit browsing to selected folder for a folder-specific scope, or Disable phone file tools. Android settings also controls the broad grant. |
| Use an SD card or document provider | Grant a folder through Android's picker. Selected-folder tools list/read that folder and create files or update text through the revision workflow. |
| Attach an image or text document | Select the document from the composer. Review the draft and attachments, then Send to the selected model. |
| Save replies and project files | Choose a WebDAV destination or selected phone folder under Storage. Save each completed reply controls autosave. Manual saves use the displayed destination. |
| Use a model connection | Configure its endpoint, model and credentials. Messages and approved tool results follow the assigned connection. Resources → Pause remote model requests keeps local file actions available. |
| Use a callable child | Link the child in the parent agent's profile and choose its model. Each request includes configured model identities and callable child indexes. Reviewed delegation returns the child's response or error to its captured parent. |
| Use web or MCP operations | Configure the service and enable the operation on the selected agent. Review the displayed arguments and destination. Treat returned material as source data. |
| Run a script | Review the submitted source and execute through Android's isolated process. Agent permissions separately control script_check and script_run proposals. |
| Start the phone API gateway | Choose its bearer token and listen address, then start the gateway explicitly. Network services → Phone API gateway → Stop gateway stops it. |

Phone listings retain up to 500 entries and text reads up to 1 MiB. The explicit composer picker, export action and reply-save workflow each use their own selected document or destination. Folder grants persist for later reuse. A browsing scope change takes effect between operations; approval rechecks the reviewed scope.

## App observations

App settings → App inspection opens the current-user app catalog. Select a package, then Read / refresh selected app details to request its version, UID, system-app flag and requested permissions with Android's PackageManager grant status. Labels come from the selected app; the exact package identity accompanies them. A mismatched permission flag array is recorded as unknown.

Exclude this app's observations saves an exact package exclusion for details and usage collection. Disable inspection and clear observations stops collection, cancels the current observation and clears the saved catalog/details while retaining exclusions. A dedicated package-query worker handles these requests; file persistence and model/network work have separate workers.

Usage access is an explicit Android grant. The selected-package view requests a time range and displays Android-reported activity resume/pause and foreground-service start/stop counts with the retained coverage. Empty results show zero matching retained records. Shared UIDs are displayed as Android reports them; exclusions use the selected package identity.

## Investigation and page reading

Investigation is part of ordinary command following, assigned tasks, child work and refinement. Use File investigation for an explicit selected-file inspection, comparison or capture. Optional preferences add task guidance through the same core flow.

The page reader follows ordinary HTTP GET redirects through five hops, using fresh public requests and recording the final URL. Each hop uses the existing 120-second transport deadline. Capture applies the reviewed HTML, font, dimensions, page-program option and CSS/PNG resource scope. Results retain source identity and observed coverage.

Local model-container inspection reads GGUF and selected safetensors headers, tensor descriptors, declared architecture, shapes and captured byte ranges. Selected DEX inspection reads bounded identifier tables, class/member declarations and code envelopes. These observations follow the originating task/model route after review.

Workspace → Search the web opens a direct reviewed search whose results appear in the app. A model-requested search returns evidence to its originating model and conversation. Configure sources in Network services → Web access.

## Local operation results

An owned local worker completion returns its actual output to the requesting model. A worker or reviewed WebDAV operation failure returns a structured failed result with its diagnostic. The conversation retains that result through continuation preparation errors. A fresh instruction starts a fresh bounded feedback allowance. Manual file actions display their result locally. Verified network saves retain their actual readback result when model continuation is paused.

## Network folder results

WebDAV folder rows use each resource's own href and successful display-name/type properties. Resource HTTP statuses appear with their row; property errors remain separate metadata. A fresh explicit read uses the current request and returned file bytes. New folder navigation refreshes the visible rows, and the actual request error accompanies a failed navigation. The same parser serves manual browsing, model-requested listings and child tasks.

Protocol reference: [RFC4918, multi-status responses and XML fields](https://www.rfc-editor.org/rfc/rfc4918.html#section-13).

## Source access switches

Network-drive keys/tokens and username/password credentials remain optional and separate from model credentials. A configured key selects one validated header method; clearing it restores the retained username/password method. Authentication errors never automatically resend using another stored credential. Configuration exports omit these secrets, and pending transfers remain bound to their original saved connection.

Microphone settings are available under App settings → Voice and microphone even with no AI or speech service. Preferences do not start capture, grant permission or issue voice consent. The explicit app-permission control opens Android's settings for this package. Automatic conversation still requires a reviewed session; background listening and spoken interruption remain separate options. No local speech-recognition model is bundled.

Workspace groups browsing under Storage and connection/account configuration under Setup. Phone, network, document-provider and MCP switches govern actual operations and owned results. Turning a source off retains its configured connection. Operations from an affected source are stopped or their returned data withheld; other sources and local workspace files keep their current permissions. Document providers use Android's generic picker and account setup. Configure accounts in the selected provider's app when it does not use Android account settings. No particular provider or key is required. New workspaces create separate app-owned directories and retain original project files. Workspace switches preserve the active-operation boundary and retire earlier document-selection callbacks.

Document selection, text-read and grant-cleanup callbacks are bound to their current operation and workspace. Cancellation and application recovery retire that ownership. Delayed selection can complete alongside model work; a stale callback preserves the newer task and form and cannot take a persistent read grant. Document response fields stay separate from editable settings. Local Android file authorities require phone access; other providers require document-provider access. Selecting a local file through Document providers requires both switches. Turning off provider access does not disable ordinary local file tools.

Remove selected document grants disables provider access and releases only tracked standalone read grants. It preserves writable grants, tree grants and the selected phone folder. Failed releases stay recorded for an explicit retry. Existing older selections are included automatically, but historical grants for files no longer recorded by the app are not broadly revoked. Android's app settings can revoke all application grants if needed. The picker supports temporary grants when a provider does not offer persistence; access may then require selecting the file again after process loss or restart. Application-owned cloud authorization and SMB remain open capabilities.
