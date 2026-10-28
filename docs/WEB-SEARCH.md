# Harness web search

Web search belongs to RLLM, not to the connected model provider. Use it with a local model, an OpenAI-compatible service, Messages/Anthropic, Content, or another supported connection. The model proposes a query, RLLM performs the approved HTTP request, and the returned evidence goes back to that same conversation and model route. Recursive tasks and assigned subagents use the same implementation. A secondary model is optional.

## Default setup

In **App settings → Network services → Web access**, leave **Custom search URL** blank. No search account, additional API key, Node program, browser installation or external computer is required. The active agent needs `web_search` in its existing tool permissions. Optional provider-hosted search in a connection is a separate explicit choice; leave that option off to use harness search.

The built-in plan queries **Mwmbl's anonymous search API**, then **DuckDuckGo HTML** if the first source is unavailable, unusable or returns no results. One approval covers the displayed finite source plan and exact query. Each source is attempted at most once; no blocked-source retry, challenge solving, query rewriting or identity rotation occurs. The two attempts use the existing per-request transport deadline of 120 seconds each. Cancel stops the current request and invalidates later callbacks.

Built-in search sends the query to the source. It does not send model API keys, custom search credentials, conversation history or phone files to either search source. The selected model receives the returned evidence through the existing conversation flow.

## Results and focused research

Built-in results contain source-provided titles, original page URLs and snippets, plus the source and attempt observations. These are search results; a snippet does not mean the complete page was fetched. Use `web_read` for a known page. Returned content is untrusted data and cannot grant permissions or execute operations.

Use focused queries for individual questions. For example, search **The Battle of Polytopia**, then read the official site and investigate separate questions about mechanics or supported platforms. The exact long all-keywords query from the reported screenshot returned zero results in a live API check, while the focused title query returned the official site and other sources. RLLM preserves the actual query and never relabels a different search as its result.

A valid empty result set is returned as **zero results**, with source observations, so the model can propose a better query through the existing review flow. If every source is unusable, the app records the actual failure. A verification page, navigation-only page or malformed response is not presented as search evidence. A failed source does not disable chat or unrelated tools.

## Source limits and custom services

Mwmbl currently documents anonymous limits of one request per second and 1,000 requests per month per IP address. The service controls availability and quota; shared networks can share the allowance. RLLM retains attribution and the service's CC-BY-NC-SA 4.0 notice with its returned data. Search coverage and snippet freshness vary. No live search source can guarantee every query or page is accessible.

An explicitly configured custom URL remains pinned to that service and its separately configured key/header. It does not silently switch to public sources. Existing HTML, text and JSON custom responses remain supported as untrusted custom-source content rather than claimed validated structured result lists. Explicitly selecting a known built-in endpoint selects that single source. Leave the custom URL blank for the default two-source plan.

MiniMax hosted search remains available as a separate optional connection feature. It is not required for local Qwen, Anthropic or any other supported model to use this harness search path.

## Implementation and verification

Original Rust code owns the search plan, request construction, bounded result parsers, callback identities and model continuation. The existing Android network binding transports the HTTP wire. No external search program or upstream engine code is installed. Result parsing and model context remain bounded; the result includes retention limits rather than claiming all index results were supplied.

Current implementation checks and the delivered APK identity are recorded in the release notes and private project checkpoint. Controlled host checks and live service probes are distinct from installed Android validation.

References: [anonymous API](https://developer.mwmbl.org/), [service terms](https://developer.mwmbl.org/terms/), [result license](https://creativecommons.org/licenses/by-nc-sa/4.0/).
