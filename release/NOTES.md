# RLLM Lab — phone access, app inspection and tool-response handling

Disclosed test build 0.1-lab.1793127500, package com.local.client.lab; Android 8.0+, ARM64 and x86-64. Install over existing Lab without uninstalling or clearing data. Previous releases remain available.

## Changes since the previous public release

- User-controlled broad shared-storage request, selected-folder mode and phone-tool off control; actual Android grants are checked. No automatic scan or access to other apps' private sandboxes.
- Explicit app catalog and selected-package version/UID/requested-permission metadata. Exclude sensitive packages before reading details; disable inspection to clear current observations. No automatic model sharing, usage/traffic collection or spyware verdict.
- Malformed leading tool-call markup now produces a clear scoped format error with raw-response inspection. Embedded JSON is never repaired/executed, and no automatic retry occurs. Valid bare-JSON proposals retain review and permissions.
- Footer/menu spacing, REA workflow description, explicit model/server metadata reads, bounded MCP negotiation/session/control/GET/resume/cancellation, unique outgoing string IDs and typed content/schema/notification checks.
- Existing provider streaming, profiles, history/Trash, appearance, reviewed delegation, adopted lessons, analytics and storage remain.

See [Access and data controls](../docs/ACCESS-AND-DATA.md). Powerful capabilities are deliberate features with effective controls; unintended access/disclosure remains a defect. Remote model requests leave the phone. Trust both your model software/runtime and the selected endpoint.

## Validation and limits

209 Rust checks are defined; no full-suite pass is claimed for this release. Independent review passed 20 focused Rust checks, eight app C ABI groups, one actual Java/Core/JNI/Rust collector group, four malformed-markup C ABI groups, one actual JVM executor group and seven unchanged interface groups. Independent reconstruction matched all 24 APK runtime payloads. Stable signer, v2/v3 signatures, CRC and ZIP/ELF 16 KiB alignment passed. APK full raw Drive readback matched.

This is not production or whole-project acceptance. Current installed Android/Samsung permissions, visual/IME/TalkBack/lifecycle/Keystore, live providers/MCP/NAS and private-app inspection are not certified. The unchanged archive-import repair remains independently unaccepted after an earlier automatic review rejection; these scoped checks do not replace that review. Preserve original backups and use disposable data for import testing.

Full MCP conformance/OAuth/newer transports/built-in MCP/ACP/A2A, streaming ZIP export with matching import, SMB/Google authorization, local compiler/inference and broader resource/accessibility work remain open. No iOS or voice build, automatic weight training or background learning is claimed.

## Artifact identity

RLLM-Lab.apk: 5892057 bytes; version code 1793127500.
SHA-256: `f806a70d4dd94d37c783e2f7e501dde8a23fc31f7e6e177457abe14d6f9dec27`
Signer SHA-256: `a36bcaabf60626403ccffd32aaf442182efcd3cae82104f2e17c0cfc344749a2`.

The release tag and attached source archive match the public source. Private checkpoints, personal screenshots, credentials, toolchains and signing material are excluded. No project-wide license grant is invented; existing notices are retained.
