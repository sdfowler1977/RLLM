# RLLM 1795240001 — Android conversation menu

Android only. The Windows installers listed below are unchanged.

The + button opens three choices: New conversation, Attach image or file, and More options. More options opens the conversation menu. A saved agent profile belongs to one LLM and is not offered on another. A stream limit is the number of streams that connection can run at once.

Package com.local.client.lab, Android 8.0+, ARM64 and x86-64, version 0.1-lab.1795240001. Same signing certificate (SHA-256 ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5), v2 and v3 signatures, and 16 KiB alignment. APK 4855757 bytes, SHA256 062b479e3d18832efc4931c1230593c38499bdc20ad5961a379c62e72a26fb37.

Installed device acceptance of this APK remains unverified.

# RLLM for Windows — installers and x64 build

RLLM now runs on x64 Windows PCs as well as Windows on ARM. `RLLM-Setup.exe` is one installer for both: it reads the machine's native processor and installs the ARM64 or x64 build, so ARM PCs get the native build rather than an emulated one. `RLLM-Windows-x64.zip` carries the smaller x64-only installer with a read-me and the public certificate, for sending to x64 PCs. Setup installs for the current user only and needs no administrator rights. It adds Start menu and desktop shortcuts with the RLLM icon and an entry in Installed apps. If RLLM is open, it asks you to close it before replacing it. Removing RLLM deletes the program and shortcuts and keeps conversations and settings in `%LOCALAPPDATA%\rllm`. The executables now carry the RLLM icon and version information. The Android package on this release is now 1795218000.

All files are Authenticode-signed by RLLM with an RFC 3161 timestamp.

| File | Bytes | SHA256 |
|---|---|---|
| RLLM-Setup.exe (ARM64 and x64) | 23338200 | b6c704b38d7475dd9ebf54f9e78fb77a3f84796c0e683a32c36e91000d39b338 |
| RLLM-Windows-x64.zip | 5552149 | 92f984254a06d9dac53ab871f751cf0423d356dc9b71153f5b8161adc72ec525 |
| RLLM-Setup-x64.exe | 12971224 | 539b0bb0244f4dc1c6621e905729fe0d780b64d0d921b60347e228f8a2e7d5da |
| RLLM-Desktop-arm64.exe | 10367192 | d9f30977daaf0917ae86365c5ab08bb69f452dc8e00730b41919ff929fa66227 |
| RLLM-Desktop-x64.exe | 12254424 | 8a9d8d392973104dc303f9e2bab3d5d455a6308d331efd66b57bdf19af9dd54b |

Install, reinstall over a running copy, uninstall with data kept, and both installers were exercised on a Windows 11 ARM64 PC; the x64 build ran there under Windows x64 emulation. Installation on a physical x64 PC and independent acceptance remain unverified.

# RLLM 1795218000 — Android + menu

Android only. The Windows installers listed above are the same files.

The + button opens the same choices as Windows: New conversation, Attach image or file, and More options. On a screen too short to show the model name, the menu also offers Change chat model. This build also carries Loop and Goal per project, the medical check, connection role and stream limits, and parallel text sub-agents.

Package com.local.client.lab, Android 8.0+, ARM64 and x86-64, version 0.1-lab.1795218000. Same signing certificate (SHA-256 ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5), v2 and v3 signatures, and 16 KiB alignment. APK 4745165 bytes, SHA256 9043fd1b59eb8acc2fda8195d7b54e1659174e6e095a165bd7d84a0197df154b. The previous Android file remains on this release as RLLM-Lab-1795217278.apk.

Installed device acceptance of this APK remains unverified.

# RLLM 1795217278 — smaller Android package

Android only. The Windows installers listed above are the same files.

Package com.local.client.lab, Android 8.0+, ARM64 and x86-64, version 0.1-lab.1795217278. Same signing certificate (SHA-256 ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5), v2 and v3 signatures, and 16 KiB alignment. APK 4708301 bytes, SHA256 fef5d51f3796f7d176547d95955fade26ad0c11059b8ca5c92d520dcde89afac. The previous Android file remains on this release as RLLM-Lab-1795208207.apk.

Installed device acceptance of this APK remains unverified.

# RLLM 1795208207 — signed candidate

RLLM now runs as a native Windows on ARM desktop app built from the same Rust core as the Android app. The window, navigation, conversation actions, history, settings and workspace are drawn by the core itself, with no browser shell and no console window. Model connections over HTTPS work on Windows through the system TLS stack with system certificate validation. Streaming replies from MiniMax now complete normally: MiniMax repeats its final completion marker with the assistant role, and the shared stream reader accepts that exact repeat while still refusing new text, a different role or a changed completion reason after a reply has finished. This fix is in the shared core, so it applies to the Android app as well. The desktop self-test now runs against a scratch vault and never touches saved history. The Android package can now also be built on a Windows host with `package.cmd`, which follows the `package.sh` steps and signing checks.

Package com.local.client.lab, Android 8.0+, ARM64 and x86-64, version 0.1-lab.1795208207; retained signing identity (certificate SHA-256 ba3d016eb002221f6305378f9efe9b7dd9531a525bad8044cedd2cf63a3dbbd5), v2 and v3 signatures, unchanged build inputs and 16 KiB alignment verified. APK 7940042 bytes, SHA256 fe9cf8e7670f901fae00ca3f4b82c049c5884397bd2373a45b96073764f16224.

Windows ARM64 executable RLLM-Desktop-arm64.exe, Authenticode-signed by RLLM with an RFC 3161 timestamp. 9834200 bytes, SHA256 7dee93c47092328ca96729a9c2d42d28a91718821c6630d10f27391d595fba73.

Installed device acceptance of this APK and independent acceptance of the Windows app remain unverified.

# RLLM 1794270237 — previous signed candidate

Auto chooses a direct reply, a short current-fact lookup, or deeper investigation. The conversation has explicit mode choices. Simple local time and date questions use the device clock. Quick lookups keep full source and error details available on tap, and a verified DJIA daily close is labeled with its observation date. Automatic web access now includes ordinary public HTTPS pages and marketplace searches opened directly by primary or delegated agents. The app checks the resolved address before connecting and applies the same public scope to every redirect. Requests for private destinations, page scripts and captures follow existing review controls. Supported OpenAI, xAI, Anthropic and Gemini connections can use provider-hosted search on both the primary agent and delegated agents. Provider searches run on the selected connection, and RLLM keeps returned source metadata. One evidence-only follow-up can recover an empty answer after search without repeating the query. Automatic web search approval is on by default for configured search sources. A single valid search proposal in model prose runs under the same approval policy, and eligible ambiguous proposals get one bounded model correction. Web-search fallback and subsequent queries stay in the current turn. Chat and local streaming enforce declared lengths and encodings and validate chunk boundaries and trailers before adopting text or completion. Bodyless HTTP 204 responses cannot publish reply text. Owned responses enforce the shared 16 KiB trailer limit. Framed MCP control acknowledgments settle after informational headers without waiting for a close. An explicit search deferral requires fresh approval even when automatic or saved approval is enabled, and omits the provider-hosted search tool for that turn. RLLM offers a second bounded refinement round after a comparable partial gain, keeps the stronger tested instructions, and compares JSON values in separate evaluation. Shared response framing accepts split delimiters at the exact 8 KiB chunk-size and 64 KiB response-header limits. RLLM can evaluate the original and retained agent instructions on up to two separate tasks with local phrase, exact-text, JSON or digest checks. Both retained replies must pass comparable checks before the evaluation can contribute a measured lesson. Evaluation proposals, interrupted responses and incomparable contexts cannot promote a lesson. A completed training candidate remains reviewable after an interrupted evaluation. RLLM learns the default built-in search order from validated same-query fallbacks. Checked agent refinements replay the exact revised instructions and retain a passing candidate when a revision fails. User adoption preserves the original agent profile. Duplicate correlated MCP POST stream replies are refused before result adoption. RLLM retains multiple named MCP connections and uses one selected server for discovery, calls and streams. A refresh publishes only the complete tool catalog and refuses duplicate tool names across pages. Ambiguous names reported by the active connection require a successful refresh before use, including after switching away and back. New conversation leads the plus menu. An explicit request to search the web authorizes built-in queries for the current turn without repeated prompts, including a follow-up command that relies on the current topic. An unavailable built-in source can fail over, and raw search diagnostics stay in message details. Higher-priority destination review remains available. Conversation displays compact web results and keeps raw tool JSON in message details. Storage authorization uses the configured certificate hostname on the matching HTTPS route while separate origins retain their own identity. Shared Rust HTTP request and response handling validates field syntax, credential-header boundaries, gateway request lengths and expectations, informational responses and chunk extensions. Optional pre-filled device verification links retain the manual address and code and fall back on unusable links. Safe WebDAV updates also preserve valid UTF-8 opaque ETags in exact conditional writes while retaining weak, malformed and duplicate validator refusals. Shared token adoption refuses non-text refresh credentials before replacing saved grants and preserves scoped uncertain-refresh recovery. Existing conversation, intrinsic investigation, local storage and optional provider-neutral services remain integrated. The signed candidate matches the recorded cumulative source inputs. Recursive refinement can check individual JSON fields in training and held-out tasks. It keeps expected values local and only treats a partial score increase as progress when earlier passing checks still pass. WebDAV listings respect declared XML namespaces, preserving valid file and folder routing when extension elements use similar names. An owned media feed failure retires its ticket before it can mint a completion receipt, including when the HTTP framer has reached its last byte. Structured refinement can unwrap a single JSON code block for local verification and records the repair separately. A failed owned import cannot issue a receipt for its staged prefix. A failed upload read retires only its own ticket before another read can advance it. Media chunks reject malformed Base64 instead of accepting a decodable prefix.

64/65 connected groups passed; 4228 executed checks and 513 Rust cases from the full suite. Incomplete groups are explicitly recorded in verification/RESULTS.json.

Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and whole-product independent acceptance remain unverified. Physical background/screen-off and battery behavior remain unverified. Bundled neural speech recognition and a separate medical application are not implemented. Physical effectiveness of echo control and simultaneous capture/playback remain unverified on a device. The historical revision-two comparator is unavailable; its three compatibility checks remain open. Scene preview displays static triangle geometry with base and vertex colors. Embedded PNG base-color textures and MASK cutouts are displayed. External images and other image codecs retain geometry-only viewing; animation and physical material effects remain open. Skins, morph targets, blended transparency and required unsupported extensions refuse only preview; export remains available. GPU rendering and wider scene formats remain open.

Package com.local.client.lab, Android 8.0+, ARM64 and x86-64; retained current signing identity, APK CRC, exact build inputs and 16 KiB alignment verified. APK 6932498 bytes, SHA256 86b7d74156a11dc6ee6d70866bb480e30b75bac57c3400878c86b9f205921c3d.
