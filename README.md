# RLLM

**Your models. Your agents. Your workspace.**

A compact Android harness for local and hosted language models. Keep a conversation while switching connections, choose named agents, route subagents and compaction separately, and inspect measurements collected by the app.

![RLLM chat and analytics illustration with fictional data](docs/images/overview-preview.png)

*Illustrative preview, not a device screenshot. Layout and example values are illustrative; no personal addresses, conversations, or performance claims are shown.*

## Install

[Download the current Android test APK](https://github.com/sdfowler1977/RLLM/releases/download/lab-1793072000/RLLM-Lab.apk) · [Setup guide](docs/SETUP.md) · [User guide](docs/USAGE.md)

The publication package also contains the actual APK at [`release/RLLM-Lab.apk`](release/RLLM-Lab.apk). The signed APK is also published as a GitHub pre-release with its SHA-256 checksum.

Requires Android 8.0 or later on ARM64 or x86-64. This is **RLLM Lab**, version `0.1-lab.1793072000`. An LLM runs on your configured server/provider; this APK does not bundle model weights.

## What you can do

- Add multiple connections with a URL, manually entered model ID, and optional API key. Local and hosted connections use the same form.
- Switch connections from the chat footer without starting a new thread. Use the pencil to edit the current connection.
- Select a named main-agent profile; set separate default subagent and compaction connections.
- Configure provider-aware thinking, output limits, and context budgets.
- Stream visible text from local chat, chat-completions, Messages and Content connections, with per-connection opt-out and complete JSON fallback.
- Explicitly refine an agent using up to four recent adopted lessons as bounded, untrusted examples, with opt-out and visible provenance.
- Manage conversation history and Trash; customize fonts, text size, colors, and background.
- Use reviewed workspace, web, storage, investigation, and delegation tools with profile-specific permissions.
- Read a quiet live `≈tok/s` estimate on the chat screen and tap it to open analytics. App observations and server-reported data remain separate.
- Save supported work to a granted phone folder or a configured WebDAV destination, including a reachable network drive.

## Start in a few minutes

1. Install the test APK, keeping an existing Lab installation and its data.
2. Tap **Connect to LLM**, or the pencil if already configured.
3. Enter a connection name, exact API URL, model ID, and key if required.
4. Choose the API format supported by your server. For an OpenAI-compatible chat server, choose **OpenAI-compatible chat** and use its `/v1` base URL.
5. Save, send a short message, and use the footer to change the connection or main agent.

See [Setup](docs/SETUP.md) for HTTP versus HTTPS, provider formats, storage, and troubleshooting.

## Documentation

| Guide | Contents |
| --- | --- |
| [Setup](docs/SETUP.md) | Installation, connections, model routes, storage, web access |
| [Daily use](docs/USAGE.md) | Chat, profiles, history, appearance, tools, refinement |
| [Analytics](docs/ANALYTICS.md) | Local rate formula, timing, errors, reported metrics |
| [Build](docs/BUILD.md) | Toolchain, native build, signing, core checks |
| [Architecture](docs/ARCHITECTURE.md) | Rust core, Android bindings, effects and permissions |
| [iOS assessment](docs/IOS.md) | Reusable code, required port, build/distribution prerequisites |
| [Test-build notes](release/NOTES.md) | Exact artifact, validation, known review status |
| [Publication](docs/PUBLICATION.md) | Repository and direct-release publication steps |

## Development status

The Android app is an active test build. The current source defines 155 Rust checks; 138 non-archive checks passed with 17 filtered out. Separate Content streaming review and exact reconstruction of all 24 APK runtime payload members passed, including both supported native architectures, DEX and resources. Archive-import repair independent review, live-provider and current Android device/visual/accessibility acceptance remain outstanding. See [release notes](release/NOTES.md) before testing project import.

iOS is not built or distributed yet. Voice conversation/open microphone is future optional work. The refinement workflow explicitly captures recent adopted lessons for proposal/composition when enabled. It remains bounded; no automatic background learning, weight training or multi-round parallel refinement is claimed.

## Source and notices

The application core is Rust, with Android Java bindings and a small JNI bridge. There is no browser/WebView UI. The public package omits private configuration, credentials, internal work logs, and personal screenshots. Required asset/runtime notices are retained in [`notices/`](notices/) and described in [asset provenance](docs/ASSETS.md). A project-wide reuse license has not been selected; no new license grant is implied by publication.
