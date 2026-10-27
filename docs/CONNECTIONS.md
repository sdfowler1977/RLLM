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

If a proxy needs query parameters, enter the **complete API method path** before `?`; automatic base-path completion with queries is not supported. Nothing retries a failed request over a different scheme automatically.

## Web search without an API key

Web access → leave **Custom search URL** blank to use DuckDuckGo HTML through the native Rust HTTP path. No search key or secondary model is needed. Normal questions and recursive tasks use the same path. Optional custom services still accept a URL containing `{query}` and separate credentials; those credentials never go to built-in search or ordinary page reads. A returned verification page is a failed retrieval, not search evidence.
