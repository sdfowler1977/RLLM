# iOS port assessment

**No iOS app or IPA has been built.** The available build environment is Linux, without Xcode, an iOS SDK, an Apple signing identity or an iOS simulator/device. The Android APK cannot be installed on iOS.

## What can be reused

The original Rust JSON model, provider adapters, conversation events, routing, bounded agent logic and measurement arithmetic provide a useful shared core. The exported `client_process` / `client_free` C interface can form a native Apple bridge after target compilation and memory/lifecycle validation.

The current UI is written with native Android Java views. No Dazzle UI definition, `.dzv3` project, iOS renderer or Dazzle runtime is included in this source. Dazzle's possible portability does not turn the existing Java views into iOS views. An available Dazzle source/runtime would need its actual iOS support inspected before choosing it; otherwise a product-owned native SwiftUI/UIKit front end is a possible path. Neither path is implemented here.

## Required work

| Area | iOS implementation needed |
| --- | --- |
| UI | Chat, composer, history/Trash, connection forms, profiles, settings and analytics with Dynamic Type, VoiceOver and keyboard handling |
| Core bridge | Build Rust for device/simulator; own UTF-8 buffers and release responses exactly once; serialize state changes appropriately |
| Network | Platform transport, TLS, streaming/cancellation, local-network access and explicit supported HTTP policy |
| Storage | Keychain-backed secrets/state encryption; Files/document-picker permissions and supported remote transfers |
| Archives | Replace Linux/Android-specific descriptor constants/layouts with reviewed Darwin bindings; do not silently use a weaker path |
| Execution/gateway | Redesign Android isolated-process/service assumptions for iOS lifecycle and execution restrictions |
| Packaging | Xcode project, bundle identity, entitlements, icons, signing/provisioning and install/distribution workflow |
| Acceptance | Real simulator/device compilation, chat/streaming/storage/restart/cancellation checks, visual and accessibility validation |

Rust publishes `aarch64-apple-ios`, `aarch64-apple-ios-sim`, and `x86_64-apple-ios` targets. Their availability is not proof that this particular application compiles or functions on iOS. The targets require the relevant Apple SDK. TestFlight/App Store distribution also requires the appropriate Apple account, signing and provisioning setup.

## Practical sequence

1. Obtain a compatible Mac/Xcode environment and the intended Apple distribution account.
2. Audit the actual Dazzle iOS backend if that is the selected UI technology; otherwise implement a native Apple front end against the C boundary.
3. Compile and exercise the reusable core on the simulator, repairing platform assumptions in place.
4. Wire real chat, encrypted persistence, file access and cancellation end to end.
5. Validate device behavior and accessibility, then archive/sign and distribute through the selected Apple channel.

This is an engineering assessment, not a starter app presented as complete. No blank iOS project or untested UI shell is included as an installable deliverable.

## Primary references

- [Rust Apple iOS target requirements](https://doc.rust-lang.org/rustc/platform-support/apple-ios.html)
- [Apple Xcode platform and host support](https://developer.apple.com/support/xcode/)
- [Apple distribution overview](https://help.apple.com/xcode/mac/current/en.lproj/devac02c5ab8.html)
- [Apple TestFlight distribution](https://help.apple.com/xcode/mac/current/en.lproj/dev2539d985f.html)

Reference review: October 26, 2026. Check current Apple requirements when the port is built.
