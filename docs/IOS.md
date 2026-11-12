# Apple platform port plan

The Rust JSON model, provider adapters, conversation events, routing, agent process and measurement arithmetic form the shared core. The exported client_process / client_free C interface provides the native bridge boundary. The Android front end uses native Java views; the Apple port calls for a product-owned SwiftUI/UIKit front end against that boundary.

## Port sequence

| Area | Implementation step |
| --- | --- |
| UI | Chat, composer, history/Trash, connection forms, profiles, settings and analytics with Dynamic Type, VoiceOver and keyboard handling |
| Core bridge | Build Rust for device/simulator; own UTF-8 buffers and release responses exactly once; serialize state changes appropriately |
| Network | Platform transport, TLS, streaming/cancellation, local-network access and explicit supported HTTP policy |
| Storage | Keychain-backed secrets/state encryption; Files/document-picker permissions and supported remote transfers |
| Archives | Adapt descriptor constants/layouts through reviewed Darwin bindings while preserving the existing scope and ownership rules |
| Execution/gateway | Redesign Android isolated-process/service assumptions for iOS lifecycle and execution restrictions |
| Packaging | Xcode project, bundle identity, entitlements, icons, signing/provisioning and install/distribution workflow |
| Acceptance | Real simulator/device compilation, chat/streaming/storage/restart/cancellation checks, visual and accessibility validation |

Build device and simulator targets using a compatible Mac, Xcode and Apple SDK. Set the bundle identity, entitlements, signing and provisioning for the chosen distribution account. Validate chat, incremental responses, encrypted persistence, file access, cancellation, accessibility and lifecycle behavior on the intended simulator and device, then archive and distribute through the selected Apple channel.

## Primary references

- [Rust Apple iOS target requirements](https://doc.rust-lang.org/rustc/platform-support/apple-ios.html)
- [Apple Xcode platform and host support](https://developer.apple.com/support/xcode/)
- [Apple distribution overview](https://help.apple.com/xcode/mac/current/en.lproj/devac02c5ab8.html)
- [Apple TestFlight distribution](https://help.apple.com/xcode/mac/current/en.lproj/dev2539d985f.html)

Reference review: October 26, 2026. Check current Apple requirements when the port is built.
