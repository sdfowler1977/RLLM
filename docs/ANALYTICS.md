# Understand the measurements

Tap the chat footer's `≈tok/s` number, or open **App settings → Diagnostics → Phone observations**.

## Live app estimate

```text
estimated tokens = received response-text Unicode characters / 4
estimated tokens per second = estimated tokens / elapsed request seconds
```

The app counts Unicode scalar values in received response text and uses Android's monotonic clock. A foreground clock refreshes the running request average every 500 ms, including periods when no new text arrives. The rate includes request waiting and network time. Completion uses the transport's recorded end time. It is not an exact tokenizer count or a measurement of the server's internal generation speed.

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

Charts retain a bounded recent request history. Select a range or model/endpoint and tap a request for details. Missing observations remain gaps rather than invented zeroes.

## Optional server monitor

Configure the exact compatible metrics JSON URL under **Server monitor**, with a separate key if needed. Refresh is manual. Server CPU/GPU/RAM, queue, engine token rate and power are labeled server-reported data. They are not independently measured on the phone, and stale samples remain identified. The main chat estimate does not depend on this monitor or on Strata specifically.
