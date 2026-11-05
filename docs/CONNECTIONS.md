# Remote LLM setup and connection troubleshooting

For a private LLM server away from home, connect the phone and server through **a secure VPN or tunnel**. A private network connection is one option, not required for a public hosted API or a server already reachable on your LAN.

1. Configure your chosen private network on the phone and server.
2. Copy the server address that is reachable from the phone.
3. Start the LLM API server and allow its listening port through the server firewall and private-network access rules. A service listening only on 127.0.0.1 cannot receive a direct connection from your phone. Restrict access to the intended devices; do not expose an unauthenticated model server publicly.
4. In RLLM, enter the server address, port, exact model ID and matching API format. Example only: `http://100.100.20.30:8080/v1` for an chat-completions API. A Local API server may instead use `http://100.100.20.30:11434/api/chat`. Use your own server's address and port.

## HTTP versus HTTPS

**Use the scheme the server supports.** A private tunnel provides a network connection; it does not turn an HTTP-only LLM service into an HTTPS service. An HTTPS URL pointed at an HTTP-only port commonly causes a TLS/SSL error.

RLLM now defaults a bare private-network IP to HTTP and preserves an explicitly entered `http://` or `https://`. Public chat-provider names still default to HTTPS. For private DNS or other hostnames whose transport is unclear, enter the full scheme explicitly. Existing saved HTTPS addresses are not changed automatically: use the pencil and correct an HTTP-only server to `http://`.

| Symptom | Check |
| --- | --- |
| TLS/SSL connection error | HTTP/HTTPS mismatch, or a real certificate/hostname problem. Keep HTTPS when the service supports it; do not ignore certificate errors. |
| Timeout or refused connection | Both devices online, VPN connected, correct server IP/port, service running/listening, firewall and access rules. |
| 404 | Wrong API format or URL path. A 404 does not prove the API key is valid. |
| 401/403 | Server authentication/authorization or access policy. |
| Model not found | Use the exact model ID served by that endpoint. |
| Choice data followed its completion | Response-stream format error: the app received a choice after its completion marker. Empty completion/usage trailers are supported; new answer data after completion is still refused. This message does not establish a Wi-Fi or firewall problem. |

If a proxy needs query parameters, enter the **complete API method path** before `?`; automatic base-path completion with queries is not supported. Nothing retries a failed request over a different scheme automatically.

Connection advice is shown for failures before sending or receiving data. Once a request has been sent, the app reports the actual failure without appending a generic LAN/firewall diagnosis. TLS failures retain scheme and certificate guidance.

## Web search without an API key

Web search is performed by the harness for any supported local or remote model. In Web access, leave **Custom search URL** blank for the built-in Mwmbl anonymous API and DuckDuckGo fallback. No search key, secondary model or external computer is needed. Normal questions and recursive tasks share the same result path. Optional custom services keep their exact `{query}` URL and separate credentials, with no automatic switch to another destination. See [Harness web search](WEB-SEARCH.md) for result handling, focused queries and source limits.

## Server-hosted Messages search

Server-hosted search is a separate capability of the server you configure. A chat connection or account key does not imply that capability is enabled. Choose the Messages API format, enter the exact server or proxy endpoint, and select Messages hosted search. The active agent must permit web_search. There is no provider hostname requirement and custom proxy paths are accepted. Existing imported mode identifiers migrate in place; saved routes, credentials, response blocks and eligible paused continuations are preserved.

The configured server must implement the declared web_search_20250305 tool and Messages search-result/continuation blocks. The app declares that contract only in eligible operation phases and within the current operation allowance. Proposals, composition and compaction use captured evidence. Returned thinking/signatures remain private continuation state; actual visible search data supplies source evidence. Results are shown when complete. Other model, phone and workspace operations retain their existing authority and review behavior.

Continue hosted search submits the captured request and exact returned assistant blocks to the same connection. Cancellation retains received history and performs no replay. Inspect hosted search responses exposes retained raw results and citations. Local reviewed search remains separately available. A response or paid-account entitlement is not inferred from a model name, hostname or key. Set up a server adapter when your program exposes a different contract.

## Optional connection authorization

Connection authorization supports a configured public-client authorization-code flow with S256 proof, an external browser and a connection-specific bearer token. It retains the original API-key fields. See [authorization](AUTHORIZATION.md) for server registration, refresh, ownership, credential privacy and known limits.

## Certificate trust

HTTPS validates certificate chains using the device's system and user-installed CA stores and checks the configured server hostname. A private NAS or model server can use a CA installed through Android credential settings. For an IP/VPN route with a DNS-named certificate, enter its certificate DNS hostname in the existing storage field. Certificate trust follows the device owner's installed roots.

Platform reference: [network security configuration](https://developer.android.com/privacy-and-security/security-config).

## Response completion

Ordinary model, storage and web replies complete when the declared HTTP body length or final chunk and trailers arrive. Close-delimited replies complete at observed EOF. The shared Rust response framer preserves the original wire bytes for the existing provider/WebDAV decoders and callback checks. Android stream I/O and gateway connections use that framer; snapshot readback uses the same framing implementation before its existing byte comparison and verification.

HTTP304 representation headers finish at the empty header line, including original content-length, chunked and coded metadata. The original status returns through the ordinary error/result path. Bodyless replies retain an empty actual body.

Model requests, snapshot network transfers, MCP control/cleanup and gateway upstream connections use the same bounded Android resolver slot. Resolution checks the original request deadline and owner while waiting; cancellation and service/activity closure retire the waiting caller. Numeric addresses connect directly through the same checks.
