# RLLM Lab 1793159000 — provider search and stream repair

Explicit MiniMax Messages hosted search uses the selected provider connection and its existing credential. Set API format to Messages, URL to https://api.minimax.io/anthropic/v1/messages, and Provider-hosted web search to MiniMax Messages hosted search. The active agent must permit web_search. Saved connections are not migrated automatically. Search runs on the provider; no Node program, SDK or other computer is installed.

Raw search results, citations, thinking and signatures are retained in encrypted state and scoped to their connection identity. Normal chat shows visible answer text. Conversation actions offers Inspect hosted search responses. A paused provider turn offers Continue hosted search using the captured request plus complete returned blocks, with a fresh request ticket and current context/resource/ownership checks. It does not replay a phone operation. Hosted mode waits for complete JSON; other connections retain their stream choice. Local search/read are not automatic fallbacks, and other phone tools still require review.

The Chat stream decoder accepts empty completion/usage trailers but rejects actual new data after completion. Valid text from the same read as a later protocol error remains as interrupted history. Repeated text is never silently deduplicated. Sent/received failures no longer receive generic LAN/firewall advice.

Independent source checks cover request and continuation budgets, worker-only routing/credentials, raw history, cancellation, stale callbacks, approval and local fallback isolation. Two actual local TCP exchanges reproduce the provider contract; no live MiniMax entitlement or provider call is claimed. Current free-search, web, format and stream regressions pass. All24 runtime payloads match an independent Android rebuild. All81 build inputs remained unchanged; stable Lab signer, v2/v3 signatures, CRC, ZIP16KiB and both ELF alignments pass.

Install over existing Lab without uninstalling or clearing data. com.local.client.lab / 0.1-lab.1793159000. APK 6223833 bytes, SHA256 67cfc900cf9a464e2a01317182fe63e0894673b5b5d69614bde309823e757171.

This is a disclosed test build. Installed Android visual/lifecycle/Keystore/live-provider and full-project acceptance remain open, as does the unchanged archive independent-review restriction. The full original native harness and browser roadmap remains active. Previous releases are retained.
