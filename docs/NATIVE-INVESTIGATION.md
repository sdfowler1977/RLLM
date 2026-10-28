# Finding and using native investigation

The native engines are Rust modules linked into the app's `libclient.so`, loaded by the existing Android/JNI binding. There is no separate REA executable, binary path, native process to launch, or native `discover` handshake. `rea` is the optional remote MCP operation; it does not invoke the built-in engines. An empty workspace means no input files are present, not that the APK lacks its native code.

The harness supplies compiled capability facts to ordinary and recursive model requests, even when optional additional guidance is off. The same facts are visible at **App settings → Diagnostics → Implementation status**. The prior status modal had no navigation button; this build fixes that route.

Ask ordinary questions about what you want to understand, build or debug. The model should locate the relevant input with permitted `list`, `read` and `search`, use `inspect(path)` for structural observations or `compare(path,other)` for byte comparisons, and base the next step on actual results. These are internal operations; you do not need to name a REA skill.

For a tool proposal, the model's entire response must be bare JSON, for example `{"tool":"list","args":{"path":"."}}`. No prose, Markdown fences or XML wrappers may accompany the proposal. An empty list calls for an input file or target location. It is not evidence that the built-in engine is absent. To check behavior, create or provide a known JSON/Rust/text fixture, inspect it, and compare the returned observations to the actual fixture.

Current compiled passes: JSON structure; CSV/TSV record shape; Unicode text; byte comparison; RIFF/WAVE; WebAssembly sections; Rust lexical declarations; ELF32/64 structure and linkage candidates; HTML source-order structure; CSS token structure. Static web reading returns page text, links and native HTML counts. The HTML/CSS machinery is a foundation for capture, not a completed browser renderer. Full upstream feature parity, deeper analysis, cascade/layout/painting, JavaScript and rendered screenshots remain implementation work, not capabilities to infer from the language used.

# Native investigation

Investigation is part of ordinary questions, programming, debugging and recursive tasks. You do not need to select a REA skill or supply a computer running an analysis server. Original Rust components inside the harness produce observations for the model to use.

## Implemented in this test build

- Local inspect dispatch now accepts the operation it advertises. It returns held-file byte observations plus JSON structure, strict comma/tab-delimited record shape, Unicode text and lexical heading observations.
- Native comparison reports positional byte differences between selected workspace files. Complete captures can establish byte equality; matching sampled prefixes cannot establish whole-file equality.
- RIFF/WAVE observations include chunk extents and base format fields. WebAssembly v1 observations include checked section envelopes and bounded custom names. These readers do not decode audio or execute/semantically validate WebAssembly.
- Actual results return to ordinary chat, subagent and refinement continuation paths. Existing operation review, selected-agent permissions and target/model routing apply.
- Custom investigation guidance is optional additional guidance. Turning it off does not remove the inherent investigation process. The optional remote MCP operation is advertised only when configured and permitted; it is not the native engine.

These are functional pieces of the full scope in [the capability inventory](INVESTIGATION-SCOPE.md), not a replacement for it. Deeper code analysis, runtime experiments, reconstruction engines, rendered browser capture and the remaining domains are still being implemented. No Node/npm, upstream REA program or desktop analysis executable is installed or bundled by these additions.

## Reading the observations

Inspect samples at most 4 MiB from a held file in 64 KiB reads; structural analyzers examine at most 128 KiB. Reports identify their source and complete/prefix coverage. JSON paths are escaped and bounded; scalar contents are not repeated in structural summaries. Delimited observations identify strict-dialect record counts and irregular field shapes, without guessing a header or column semantics. Binary inventories report at most 64 headers and distinguish declared extents from captured bytes. Compare examines up to 1 MiB per file. Files changed during reading are reported as a failed observation. The two comparison reads are not an atomic snapshot.

Approved findings can be sent to the selected model provider. These are local analysis engines, not a claim that all inference or task data remains on the phone.


## Rust source and ELF structural passes

Rust source files now receive comment/literal-aware lexical observations: declaration-shaped names and byte locations, unresolved use/module/impl relationships, and exact whole-input or prefix coverage. Nested comments, ordinary/raw/byte/C strings, character literals and lifetimes are distinguished. This pass does not expand macros, resolve modules/types or compile source.

ELF files receive original Rust ELF32/ELF64 observations in both byte orders: checked header/table extents, declared file-to-virtual LOAD mappings, section names, and bounded dynamic symbol and dependency-name candidates. Compressed or incomplete metadata is identified. Relocation, disassembly, dependency resolution and actual process memory remain separate implementation work. Neither pass requires an external program, Node, or a remote analysis server.

Both run through the same existing inspection path from ordinary questions and recursive tasks, returning actual observations into the originating model request. Investigation settings edit additional guidance; native investigation remains inherent when that additional guidance is off.

## Model files

Ask about a selected workspace `.gguf` or `.safetensors` file, or use the existing Inspect action. The same Rust file observer runs in ordinary and recursive requests. It reports supported metadata, architecture labels, tensor names/shapes and checked file ranges from actual bytes. Large files retain explicit prefix coverage. It does not load weights or run inference. Quantized GGUF storage lengths and unsupported dtype encodings remain explicit limits, not guessed values.

## DEX files

Ask about a selected workspace `.dex` file or inspect that file directly. The Rust observer reads supported header/map, string/type/prototype/field/method tables, declared classes and their defined members. Reports keep declared references separate from definitions. Current revisions035/037/038/039/040 have bounded declaration observations;041 container headers have a separately stated limited observation. Code envelopes are locations and sizes, not decoded instructions or decompiled behavior. APK extraction, Android resource/signature parsing and deeper code analysis remain required follow-on work.
