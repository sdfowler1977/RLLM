# RLLM Lab — provider streaming and adopted lessons

Android test build `0.1-lab.1793072000`, built October 26, 2026 (UTC−04:00; October 27 UTC). Package `com.local.client.lab`; Android 8.0+; ARM64 and x86-64 native libraries included.

## Changes since the previous public release

- Messages and Content API replies stream visible text through the existing native progress path, alongside local chat and chat-completions. Each connection can opt out; compatible complete JSON responses remain supported.
- Hidden reasoning/thought text and signatures are excluded from transcript and approximate rate. Structured provider tools/code/media remain explicitly unsupported and are never executed.
- Interrupted, cancelled, blocked, malformed or truncated streams retain received text as Interrupted. The current request's credentials, model, generation settings and role routing remain pinned.
- Explicit agent refinement can reuse up to four recent adopted lessons as bounded, untrusted proposal/composition examples. The default-on form control supports opt-out and shows captured provenance. This is explicit workflow reuse, not automatic training.
- Existing model/agent selection, history/Trash, appearance, generation/context controls, nested reviewed delegation, REA, analytics and encrypted transfers remain available.

## Validation and test status

The current source defines 155 Rust checks. The final owner run passed 138 non-archive checks with 17 filtered out. Separate scoped Content review passed 17 stream checks, five provider checks, four public C ABI/real-TCP groups and fresh related regressions. Independent Android reconstruction matched all 24 runtime payload members in this exact APK: both native/JNI libraries, DEX, manifest and resources. Stable signer, v2/v3 signatures and ZIP/ELF 16 KiB alignment passed. APK and complete private engineering source had full downloaded Drive readback verification before publication.

Archive-import repair independent review remains unavailable after an automatic review rejection; this Content/package review does not replace it. Current installed-Android visual/accessibility/IME/lifecycle/Keystore/provider, live external provider/NAS and whole-project acceptance remain open. This is an explicitly requested **test build**. Use disposable data for archive-import testing and retain original backups.

No iOS app is included. Optional voice/open microphone remains future work. Broader structured tools, multimodal capabilities, protocol negotiation, streaming project ZIP, SMB, Google authorization, local compiler/inference and the remaining harness roadmap stay open.

## Artifact identity

Filename: `RLLM-Lab.apk`  
Size: `5617625` bytes  
Version code: `1793072000`  
SHA-256: `7fad882d3fb59d795f991106fa373ba9b7070edffad6f1c604a3c688ca9616f4`  
Signer certificate SHA-256: `a36bcaabf60626403ccffd32aaf442182efcd3cae82104f2e17c0cfc344749a2`

Install over an existing Lab installation signed with this identity. Do not uninstall or clear data. The previous pre-release remains available. The new release tag is created after the source update, and the attached `RLLM-source.zip` contains the matching public source, build/setup guides, notices and fictional illustrations. Private engineering checkpoints, credentials and signing materials are excluded.
