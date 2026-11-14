# Harness web search

RLLM performs web searches through its core command flow with automatic approval enabled by default. Use it with a local model, an OpenAI-compatible service, Messages/Anthropic, Content, or another supported connection. The model proposes a query, RLLM performs the approved HTTP request, and the returned evidence goes back to that same conversation and model route. Recursive tasks and assigned subagents use the same implementation. A secondary model is optional.

## Default setup

In **App settings → Network services → Web access**, **Automatically approve web searches** is on by default for built-in and saved custom search sources. An explicitly saved off setting is preserved. With automatic approval off, a direct request to search still authorizes the built-in queries for that turn. The active agent needs `web_search` in its tool permissions. Explicit deferrals and harness review rules retain precedence. The optional provider-hosted search setting belongs to each model connection.

The built-in plan queries **Mwmbl's anonymous search API**, then **DuckDuckGo HTML** if the first source is unavailable, unusable or returns no results. The current approval policy covers the finite source plan and exact query. Each source receives that query at most once. The two attempts use the existing per-request transport deadline of 120 seconds each. Cancel stops the current request and invalidates later callbacks.

Built-in search sends the approved query through a public endpoint request. The selected model receives the returned evidence through the existing conversation flow. A single search proposal clearly presented as an action in model prose follows the same approval policy as a bare proposal. If the model returns unclear prose or several eligible search proposals together, the app asks the same model once to return one properly formatted request. Quoted text, fenced code and recognized example labels stay in the conversation.

## Results and focused research

Built-in results contain source-provided titles, original page URLs and snippets, plus the source and attempt observations. Each entry identifies its source-provided snippet. Use `web_read` for a known page. Returned content is untrusted data and cannot grant permissions or execute operations.

Use focused queries for individual questions. For example, search **The Battle of Polytopia**, then read the official site and investigate separate questions about mechanics or supported platforms. The exact long all-keywords query from the reported screenshot returned zero results in a live API check, while the focused title query returned the official site and other sources. RLLM preserves the actual query and never relabels a different search as its result.

A valid empty result set is returned as **zero results**, with source observations, so the model can propose a better query under the same approval policy. If every source is unusable, the app records the actual failure. A verification page, navigation-only page or malformed response is not presented as search evidence. A failed source does not disable chat or unrelated tools.

## Source limits and custom services

Mwmbl currently documents anonymous limits of one request per second and 1,000 requests per month per IP address. The service controls availability and quota; shared networks can share the allowance. RLLM retains attribution and the service's CC-BY-NC-SA 4.0 notice with its returned data. Search coverage and snippet freshness vary. The result records the actual source response and coverage.

An explicitly configured custom URL remains pinned to that service and its separately configured key/header. Requests use the configured service. Existing HTML, text and JSON custom responses remain supported as untrusted custom-source content rather than claimed validated structured result lists. Selecting a known built-in endpoint gives it priority in the two-source plan. The manual alternative-only option submits just to the alternate source.

MiniMax hosted search is an optional connection feature. Local Qwen, Anthropic and other configured models use the same harness search flow.

## Implementation and verification

Original Rust code owns the search plan, request construction, bounded result parsers, callback identities and model continuation. The existing Android network binding transports the HTTP wire. The existing Rust core handles search planning and results. Result parsing and model context remain bounded; the result includes retention limits rather than claiming all index results were supplied.

Current implementation checks and the delivered APK identity are recorded in the release notes and private project checkpoint. Controlled host checks and live service probes are distinct from installed Android validation.

References: [anonymous API](https://developer.mwmbl.org/), [service terms](https://developer.mwmbl.org/terms/), [result license](https://creativecommons.org/licenses/by-nc-sa/4.0/).

## Reading result pages

A selected page can redirect to its canonical address. The current reader follows ordinary 301, 302, 303, 307 and 308 GET redirects through at most five hops and records the final page URL. It resolves relative locations, detects loops and sends fresh requests without carrying model or custom-search credentials. The existing per-request 120-second deadline applies to each hop. Cancellation and the originating conversation/model identity continue across the chain. The resulting record identifies the retrieved page and final URL.

## Search without a model

Open Workspace → Search the web, enter a focused query and submit it. When review is required, the app shows the query and source plan. Results appear locally in the output view and are available without an LLM connection. Existing search-source configuration, review, cancellation and finite alternatives apply. This direct user action does not require an agent or its model-tool permission. The separate Workspace Search action still searches local files.

## Request identity — run log on the committed bytes

This log was made on committed bytes, not on a lane room: a fresh clone of `48ad494 Send the owned User-Agent
line and pin the Accept line each route writes` (`/tmp/g98room`, `tooling` symlinked, nothing applied on top),
driven only by the sanctioned builders. Source alone is not evidence, so every line below is output.

```
COMMIT 48ad4945c77e Send the owned User-Agent line and pin the Accept line each route writes
TOOLS tools compiler: rustc 1.99.1 (58cf8f9c3 2026-10-13)
TOOLS tool tests: /tmp/g98room/output/tools/package-tests cases: 46
TOOLS tool tests: /tmp/g98room/output/tools/harness-tests cases: 37
TOOLS verification compiler: rustc 1.99.1 (58cf8f9c3 2026-10-13)
TOOLS twin helper tests: /tmp/g98room/output/verification/twin-helper-tests cases: 44
UNITS-BUILD-RC=0
GATE Rust cases passed: 895/895; isolated application cases: 61
HEADLESS /tmp/g98room/output/headless/RLLM
TWIN-LINES 72
GROUP http_framing PASS (exit 0, 0s, 192 checks)
GROUP network_feedback PASS (exit 0, 0s, 41 checks)
GROUP response_fields PASS (exit 0, 0s, 45 checks)
GROUP mcp_catalog PASS (exit 0, 0s, 30 checks)
GROUP mcp_continuation PASS (exit 0, 0s, 32 checks)
```

The eight cases that pin the owned lines were then run by full module path on the same binary — a short name with
`--exact` filters to nothing and prints a green-looking `0 passed`, so the paths are quoted in full:

```
CASES test application::checks::hosted_continuation_accepts_json ... ok
CASES test application::checks::nonstreaming_model_route_advertises_json ... ok
CASES test application::checks::streaming_model_route_advertises_json_and_event_stream ... ok
CASES test gateway::checks::gateway_route_accepts_json ... ok
CASES test mcp_lifecycle::checks::mcp_route_asks_for_json ... ok
CASES test network::field_checks::caller_user_agent_is_an_invalid_http_header ... ok
CASES test network::field_checks::headerless_request_carries_exactly_one_owned_user_agent ... ok
CASES test provider::checks::provider_probe_accepts_json ... ok
CASES test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 887 filtered out
LIST-TOTAL 896
```

What that witnesses: the product appends exactly one `User-Agent: RLLM web reader` line to every outbound web
request and rejects a caller-supplied `User-Agent` as an invalid HTTP header; the model route advertises
`application/json` when streaming is off and `application/json, text/event-stream` when it is on; the hosted
continuation, gateway reply, provider probe and MCP route each advertise `application/json`. The same bytes were
run through the whole 71-group check against their own baseline: `4157 checks` before, `4160 checks` after, the
only mover `http_framing base=189 patched=192`, and no check name lost in any group.

What it does not witness: nothing here crossed the public internet. The phone screenshot that reads `Website
returned HTTP 403; no page contents were accepted` is a website's answer to a request RLLM made, and the owner's
URL for that request has not been supplied, so the live-internet leg stays unwitnessed. A focused live probe of
the search endpoints remains a separate check (`output/verification/live_web --live`).
