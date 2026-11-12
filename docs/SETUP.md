# Setup

## Workspace and storage

Open **Workspace → Projects → New workspace**, enter a name and choose **Create workspace**. Each workspace has its own app-owned project directory. **Switch workspace** returns to a saved location; **Original workspace** retains the files from earlier releases. **Project files** opens browsing, text editing, reading and search. **Project snapshots** contains import, export and saves.

**Workspace → Storage** is for browsing: **Phone locations**, **Network locations** and **Google Drive**. **Workspace → Setup** is for phone folders/scope, network connection settings, Google Drive accounts, MCP connections, access permissions and default saves.

Configure a source once, then use its access toggle to allow or suspend app operations while retaining its setup. **Access permissions** groups phone, network, Google Drive and MCP switches. Phone setup also chooses Android's broad storage grant or a selected-folder browsing scope. **Default saves** independently chooses the phone/network destination and reply autosave.

For Google Drive, open **Setup → Google Drive account → Sign in / add Google account**. Android manages the account sign-in. Then open **Storage → Google Drive → Browse Google Drive files**, choose Google Drive in Android's Locations and select a file. Use **Open selected file** for its Android viewer, **Read selected text** for a local UTF-8 preview, or **Copy text to workspace** to open the existing reviewed editor. **Remove Google Drive file grants** releases the app's saved document grants for that provider. These actions use the device's installed document provider and account controls.

## Model connections

Open the connection editor with the pencil control. Enter the server address, API format and exact model ID. Enter the provider key when the server uses one. Use the server's documented scheme, host, port and API path.

| Format | Typical request path |
| --- | --- |
| Chat completions | `/v1/chat/completions` |
| Local chat | `/api/chat` |
| Messages | `/v1/messages` |
| Content | `/v1beta/models/MODEL:generateContent` |

Choose the provider format that matches the configured server. HTTPS uses certificate and hostname verification. Explicit HTTP supports trusted LAN/VPN listeners. Thinking, output-token and context settings follow the selected model's documented parameters. Save the profile and select it for the chat.

## Agents and children

Open Agent settings → Agent profiles. Create the child profile, select its model override, instructions and permitted tools. Edit the parent profile, enable the specific Callable subagent and permit `delegate`. Select the parent agent in the chat footer.

A reviewed delegation starts the selected child connection. The task view shows the running child. Returned results and interruptions continue through the captured parent connection. User rules and Once/Session/Always approvals govern reviewed tool operations.

## Web access

Open App settings → Network services → Web access. A blank Custom search URL selects the built-in search sources. Manual Web search works directly through the harness. Models can propose permitted `web_search` and `web_read` calls, whose returned evidence continues the requesting task.

## Network storage

For WebDAV, enter the complete folder URL, username and password. New files are reported saved only after a readback matches the requested UTF-8 bytes. An interrupted or mismatched readback stays unconfirmed; no write is automatically repeated. Folder listings accept XML comments and CDATA property text while refusing document-type/entity declarations. Use the service's actual folder path and port. Review a listing, select a file and use the available read/copy/create/update/folder operations.

For a VPN IP route whose certificate names a DNS host, enter that exact host in Certificate DNS hostname. Enter the hostname itself, for example `storage.example.net`. The TCP connection uses the URL address; TLS identity, SNI and HTTP Host use the selected DNS name with ordinary chain and hostname verification. WebDAV listings may use either the configured route or this explicit certificate name; accepted entries keep requests on the original route.

## Question and design process

Open Investigation → Question and design process. Start a question, edit its plan, record evidence, add findings/design notes, assess individual questions or requirements and review finite checks. Select Use for manual observations to associate subsequent operations with the question. Export uses the reviewed workspace-file operation.

Ordinary, recursive, delegated and refinement tasks share this process. Plan changes retain assessment history and give the revised items their own review state.

## History and Trash

Use history controls to branch, restore or move conversations to Trash. Open Trash, choose Empty Trash, review the displayed conversations and confirm the exact snapshot. Keep returns to the current Trash view. Individual restore and purge controls remain available.

## MCP servers and build tools

1. Open **Workspace → Tools and servers → Server connection**, or **Workspace → Setup → MCP server connection**.
2. Enter the server's full URL, port and MCP path, for example `http://server:port/mcp`, and its bearer token when required. Set **Allow this server connection** and **Allow the selected agent to use MCP tools** as desired.
3. Choose **Save and discover tools**. The app negotiates with that server and opens **Available server tools** with the returned names.
4. Select a tool, inspect its description/schema, enter its JSON arguments and choose **Review call → Run this call**. The returned result appears in the app. Server stream controls are grouped separately under **Server streams**.

For a build server such as Dazzle, choose its advertised workspace/project creation or opening tool first, then its build tool with the parameters and paths from that server's schema/results. App workspaces hold local phone project files; server workspace parameters refer to the build server's locations.

To let the primary model use those tools, enable **Allow the selected agent to use MCP tools** in the connection form, or open **Tools and servers → Agent tool access** and enable `mcp` for the selected agent. Ask the model to discover and use the configured server. The existing reviewed command flow carries the actual tool names, arguments and results. **Setup → Access permissions → MCP server access** controls the connection independently of retained setup and per-agent tool choices.

## Connection checks

Check the scheme, host, port, path, provider format, exact model ID and key against the server configuration. For HTTPS storage, check the certificate's DNS name and trust chain. For child orchestration, check the child profile's model override and the parent's callable-child/tool selections.
# Installing this RLLM release

The launcher name is **RLLM** and the package remains `com.local.client.lab`. Builds signed with the current certificate update one another normally; do not uninstall a build already using that current signer. The user authorized a replacement signing identity. Android rejects an update signed with a different key. Export any needed conversations and configuration using the existing app before removing that installation, then install this signed APK. Uninstalling removes app-private data and its encryption key; do not rely on restoring encrypted app-private files afterward. Exported conversation/configuration files can be imported using the existing controls. No installation or removal is performed automatically.

Keep the current signing identity for subsequent releases so they can update this build normally. Its certificate SHA256 is ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5.
