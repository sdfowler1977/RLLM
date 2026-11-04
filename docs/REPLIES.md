# Reply metadata

The shared Rust provider decoder supplies text and typed completion metadata to Android and headless operations. Explicit output/context limits retain actual partial text as interrupted before tool proposals, process answers, memory reports, compaction and refinement adoption. A child interruption returns its status once to the captured parent connection.

`op:"response"` returns the existing text value. `op:"response_details"` returns this version-one shape:

```json
{"version":1,"text":"partial reply","stop_reason":"length","interrupted":true}
```

Known limit markers are chat `finish_reason:length`, local `done_reason:length`, Messages `stop_reason:max_tokens` or `model_context_window_exceeded`, and Content `finishReason:MAX_TOKENS`. Absent or unknown optional metadata keeps its existing decoder behavior. Empty limited responses retain typed interruption metadata for the application diagnostic.

Incremental replies use the same decoder at completion. Visible text decoded before a stream error is retained. Hosted Messages responses retain their raw blocks alongside presentation text. Normal completion continues through the existing reviewed tool and answer workflows.
