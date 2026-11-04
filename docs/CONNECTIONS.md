# Remote LLM setup and connection troubleshooting

For a private LLM server away from home, connect the phone and server through **Tailscale or another secure VPN/tunnel**. Tailscale is one option, not required for a public hosted API or a server already reachable on your LAN.

1. Follow the official [Tailscale quickstart](https://tailscale.com/docs/how-to/quickstart) on both devices. Keep both connected to your tailnet.
2. Copy the **server's** Tailscale IP from its device listing. See [Connect to devices](https://tailscale.com/docs/how-to/connect-to-devices).
3. Start the LLM API server and allow its listening port through the server firewall and tailnet access rules. A service listening only on 127.0.0.1 cannot receive a direct connection from your phone. Restrict access to the intended devices; do not expose an unauthenticated model server publicly.
4. In RLLM, enter the server address, port, exact model ID and matching API format. Example only: `http://100.100.20.30:8080/v1` for an OpenAI-compatible chat API. A Local API server may instead use `http://100.100.20.30:11434/api/chat`. Use your own server's address and port.

## HTTP versus HTTPS

**Use the scheme the server supports.** Tailscale provides a network connection; it does not turn an HTTP-only LLM service into an HTTPS service. An HTTPS URL pointed at an HTTP-only port commonly causes a TLS/SSL error.

RLLM now defaults a bare local/Tailscale IP to HTTP and preserves an explicitly entered `http://` or `https://`. Public chat-provider names still default to HTTPS. For MagicDNS or other hostnames whose transport is unclear, enter the full scheme explicitly. Existing saved HTTPS addresses are not changed automatically: use the pencil and correct an HTTP-only server to `http://`.

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

## MiniMax provider-hosted search

MiniMax's hosted search is a separate API capability. A chat model connection at `/v1/chat/completions` does not enable it merely because the same account can search in MMX.

In the connection editor, select **Messages** as the API format, enter `https://api.minimax.io/anthropic/v1/messages`, keep your exact model ID and existing MiniMax key, and select **MiniMax Messages hosted search** under **Provider-hosted web search**. Save the connection. The active agent must have `web_search` enabled. The app does not silently rewrite saved endpoints or move credentials to another origin.

With this option enabled, eligible ordinary, child, baseline and probe requests declare MiniMax's server search tool. Instruction proposal/composition and conversation compaction use captured evidence without declaring that tool. The app also stops declaring it at the existing turn operation allowance, and refuses a paused continuation that no longer belongs to an operation phase with room remaining. The provider controls its own infrastructure; these limits describe RLLM's requests. RLLM does not substitute its local `web_search` or `web_read`. Hosted-search requests wait for a complete response; other connections keep their streaming setting. Other phone/workspace operations still use their existing review flow. Provider access and charges depend on your account.

If the provider pauses the turn, use **Continue hosted search**. This sends the captured request plus the complete returned assistant blocks to the same connection, with a fresh request ticket. It does not replay a phone operation. Stop cancels continuation and retains received history. Conversation actions → **Inspect hosted search responses** exposes saved raw results and citations; raw thinking and signatures stay out of the normal answer display.

The provider adapter uses original Rust code and the existing Android transport. No SDK, Node package or external computer is installed. Free phone-side search remains available with hosted search off. Current implementation and test-build identity are recorded separately from live-provider/device validation.

Official interface reference: [MiniMax Server Tools](https://platform.minimax.io/docs/guides/server-tools).

## Certificate trust

HTTPS validates certificate chains using the device's system and user-installed CA stores and checks the configured server hostname. A private NAS or model server can use a CA installed through Android credential settings. For an IP/VPN route with a DNS-named certificate, enter its certificate DNS hostname in the existing storage field. Certificate trust follows the device owner's installed roots.

Platform reference: [network security configuration](https://developer.android.com/privacy-and-security/security-config).

## Response completion

Ordinary model, storage and web replies complete when the declared HTTP body length or final chunk and trailers arrive. Close-delimited replies complete at observed EOF. The shared Rust response framer preserves the original wire bytes for the existing provider/WebDAV decoders and callback checks. Android stream I/O and gateway connections use that framer; snapshot readback uses the same framing implementation before its existing byte comparison and verification.

HTTP304 representation headers finish at the empty header line, including original content-length, chunked and coded metadata. The original status returns through the ordinary error/result path. Bodyless replies retain an empty actual body.

Model requests, snapshot network transfers, MCP control/cleanup and gateway upstream connections use the same bounded Android resolver slot. Resolution checks the original request deadline and owner while waiting; cancellation and service/activity closure retire the waiting caller. Numeric addresses connect directly through the same checks.
