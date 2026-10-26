# RLLM Lab — chat analytics

Android test build `0.1-lab.1793062000`, built October 26, 2026. Package `com.local.client.lab`; Android 8.0+; ARM64 and x86-64 native libraries included.

## In this build

- A muted live `≈tok/s` number sits to the right of the centered main-agent selector; tap it for analytics.
- The estimate is calculated by the app from received response-text Unicode characters divided by four and elapsed request time. It includes network/wait time and is not exact model tokenization.
- App-estimated, measured-character and provider-reported throughput remain separate.
- Connection switching, named agents, role routing, thinking/context controls, history/Trash and appearance settings remain available.

## Validation and test status

The source passed 145 Rust checks. A separate reviewer checked the throughput/UI event behavior and compiled the Java bindings. Both native ABIs, DEX, ZIP integrity, and v2/v3 APK signatures were verified. The APK was downloaded from its Drive backup and compared byte-for-byte.

The archive-import repair's independent review remains outstanding, and this build has not received current physical-device visual/accessibility/IME acceptance. This is an explicitly requested **test build**, not a production-acceptance claim. Use disposable data for archive-import testing and retain original files/backups.

No iOS app is included. Optional voice conversation remains future work.

## Artifact identity

Filename: `RLLM-Lab.apk`  
Size: `5580761` bytes  
Version code: `1793062000`  
SHA-256: `b8c756cc2987921148521a7e018f1c2d125864adf68766ab44fcc31f14870153`  
Signer certificate SHA-256: `a36bcaabf60626403ccffd32aaf442182efcd3cae82104f2e17c0cfc344749a2`

Use an in-place update for an existing installation signed with this Lab identity. Do not uninstall or clear app data to force an update. Independently signed development builds cannot replace this signed installation.
