# Current implementation

Optional foreground open-microphone conversation now captures and segments speech, uses configured recognition and the primary model, plays configured synthesized audio, then listens again after playback completes. Generic server hosts, credentials, mappings and ordered fallbacks remain configurable. Manual recording, uploads, generation jobs, streamed artifacts and the primary/child workflow are retained. Missing speech services leave text and local work available.

Setup, mappings, job behavior and limits: [SERVICES.md](SERVICES.md). The established text, investigation, web, file, storage, MCP/A2A and delegation paths remain part of the same app.

Validation: 34/35 connected groups, 1484 executed checks, 197 selected Rust cases. Reports describe owner execution, not independent acceptance.

Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and independent review remain unverified. Simultaneous interruption while speaking, background listening, bundled neural speech recognition and a separate medical application are not implemented. The historical revision-two comparator is unavailable; its three compatibility checks remain open. Full Rust sweeps retain 14 failing historical fixtures also failing on the preserved baseline; no new fixture failures were found. Current operation equivalents are covered separately.
