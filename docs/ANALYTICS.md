# Understand the measurements

Tap the chat footer's `≈tok/s` number, or open **App settings → Diagnostics → Phone observations**.

## Live app estimate

```text
estimated tokens = received response-text Unicode characters / 4
estimated tokens per second = estimated tokens / elapsed request seconds
```

The app counts Unicode scalar values in received response text and uses Android's monotonic clock. A foreground clock refreshes the running request average every 500 ms, including periods when no new text arrives. The rate includes request waiting and network time. Completion uses the transport's recorded end time. The displayed value is a character-based approximation of received reply throughput.

The approximation symbol is intentional. Tokenization varies by model, language and code. A combining character sequence can contain several scalar values. Text-form tool request JSON contributes to received response text; hidden reasoning and structured tool-call fields do not. Neither the number of streaming chunks nor a server-reported token count controls this estimate.

A dash means there is not yet a usable text/time measurement. Starting a new request resets the live observation. Completed or interrupted observations keep the appropriate request, model and conversation identity. Nonstreaming responses do not create fake live samples while waiting.

## Other observations

| Measurement | Source and meaning |
| --- | --- |
| Request duration | App clock; includes network and queue time |
| First response byte | App-observed first incoming HTTP byte |
| First text | First received streamed reply text, when available |
| Text throughput | Counted response characters divided by measured request time |
| Estimated token throughput | Characters/4 divided by measured request time |
| Reported token throughput | Provider usage count divided by app-measured request time; displayed separately |
| Error rate | Errors among recorded success/error outcomes; cancellations excluded |
| Received/sent bytes | Application HTTP bytes; not all TLS/TCP overhead |

**Average over** selects an exact number from 1 to 128 recent runs; the presets are 5, 10, 25 and 50. The default is 10. This choice is saved with the app settings. A selected model/endpoint is filtered before the count is applied. The header shows the requested count and how many retained requests are available.

Response-time and first-text gauges show arithmetic means of successful measured replies. First-text samples also require a valid timing within the request duration. Estimated token throughput uses total observed characters ÷ 4 divided by total measured request seconds, so longer requests receive their corresponding time weight. Each gauge shows its sample count. Error rate uses successful and failed outcomes; cancellations are excluded. Charts, model statistics and the request list share the selected range. The live request has its own card and joins history when it settles.

The app retains the latest 128 requests. Tap a chart cell or open Request details for an individual observation. Missing observations remain gaps.

## Optional server monitor

Configure the exact compatible metrics JSON URL under **Server monitor**, with a separate key if needed. Refresh is manual. Server CPU/GPU/RAM, queue, engine token rate and power are labeled server-reported data. Each sample retains its server-reported origin and freshness. The main chat estimate uses the app clock and received text with any configured connection.
