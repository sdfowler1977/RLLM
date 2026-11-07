# RLLM

RLLM connects your Android device to configured language models and integrates investigation into ordinary command following.

Configured models and callable-agent routes appear in the request catalog. Reviewed delegation returns the actual child response or error to the captured parent. Question/design records connect plans, source observations, interpretations, finite checks and implementation decisions.

The app includes four provider formats, incremental replies, user agent/skill settings, file inspection/comparison, typed binary evidence, web search/page capture, reviewed reconstruction/experiments, WebDAV storage, encrypted history and local MCP/A2A access.

The source includes one matching signed RLLM APK in release/RLLM.apk. Build instructions are in [docs/BUILD.md](docs/BUILD.md), command behavior in [docs/COMMANDS.md](docs/COMMANDS.md), and recorded validation in [verification/RESULTS.json](verification/RESULTS.json).

Voice controls are in **App settings → Voice and microphone**, and are also reachable from Conversation actions and Voice and media. Save microphone preferences without configuring a model or speech server. Configure optional recognition and speech-output connections, choose **Review open microphone conversation**, review the destinations and listening options, then enable the session. A speech-output key alone does not provide recognition. Microphone capture and silence detection run in the app; speech-to-text uses your chosen server and no recognition model is bundled. Background listening remains a separate reviewed option. See [voice setup](docs/SERVICES.md#voice-and-microphone-settings).

Optional provider-neutral server capabilities now support foreground open-microphone conversation, explicit Speak now interruption, optional foreground spoken interruption with device echo control, and manual voice turns, file uploads, an interactive static GLB geometry viewport, image/video/scene/document routes, ordered fallbacks and tracked generation jobs. Setup and limits are in [docs/SERVICES.md](docs/SERVICES.md). Optional S256 browser authorization supports owned automatic credential refresh and pending-browser recovery while retaining API-key mode. Standalone read-only ACP v1 sessions use the shared workspace engine. Headless tasks discover bounded usable CPU concurrency when device metadata is absent. All extra services remain optional. Current candidate validation and any unavailable checks are recorded separately from the preserved baseline.
