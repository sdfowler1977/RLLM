# Current implementation

Optional generic server hosts, routes, credentials, request/response mappings and ordered fallbacks now connect foreground voice, file uploads, generation jobs and streamed artifacts to the existing primary/child workflow. Local work and the primary model remain usable when extra services are absent or unavailable.

Setup, mappings, job behavior and limits: [SERVICES.md](SERVICES.md). The established text, investigation, web, file, storage, MCP/A2A and delegation paths remain part of the same app.

Validation: 34/35 connected groups, 1415 executed checks, 194 selected Rust cases. Reports describe owner execution, not independent acceptance.

Installed Android microphone/playback/UI/lifecycle acceptance, real user server endpoints and independent review remain unverified. Open microphone and a separate medical application are not implemented. The historical revision-two comparator is unavailable; its three compatibility checks remain open. Full Rust sweeps retain 14 failing historical fixtures also failing on the preserved baseline; no new fixture failures were found. Current operation equivalents are covered separately.
