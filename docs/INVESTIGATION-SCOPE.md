# Universal native investigation scope

Research date: 2026-10-27. Status: required coverage inventory, not parity or implementation acceptance. This document supports the live implementation work; it is never a runtime prerequisite.

## Authority and provenance

The current first section of RULES.md and ROADMAP.md controls. Investigation is inherent in ordinary requests, programming, debugging, planning, recursive tasks, delegation and refinement. The user must not invoke a skill, enter a separate REA area, install another analysis program, or supply a computer/server. New engines and orchestration are original project-authored Rust. Existing optional remote connections and platform bindings are preserved but cannot be counted as the native implementation.

This inventory uses public behavior documentation only, including the upstream README, MCP contracts, guided-workflow descriptions, CLI behavior and domain guides. No upstream implementation source was read or copied for this inventory. Public documentation exposure is recorded honestly; it is not a claim of an independently audited clean-room process. Upstream runtime choices are contextual facts, not implementation instructions for RLLM.

The generated operation catalog is described by upstream as a deployment/build output, not a checked-in snapshot. Its two public website URLs were unavailable through the research reader in this run. Therefore this document preserves every advertised domain family found in the public overview and the cross-domain process requirements; it does not claim an exhaustive exact operation count, current generated-schema equivalence, or full parity. Before claiming parity, obtain the exact catalog for the comparison revision and reconcile every operation with this inventory. Missing catalog access does not prevent current native implementation or remove any family below.

## Current observed baseline

- `core/investigation.rs` implements bounded file sampling, signature hints, a small ELF header observation, sampled entropy, printable strings and static HTTP/HTML text extraction. These are actual limited observations, not decompilation or a complete format reader.
- Existing Rust list/read/search and reviewed write paths provide local evidence and output plumbing. Configured web and MCP operations have independent authority and result paths; an available remote engine does not establish an on-device equivalent.
- Existing harness task phases, bounded delegation, retained tool evidence, explicit refinement/adoption and shared adopted lessons supply parts of the surrounding workflow. Prompt guidance and stored examples alone do not implement universal exploration, autonomous hypothesis testing, or learning.
- The current root work is adding original bounded JSON/text structural inspection through a new exploration module and connecting it to inspection. That work is **under implementation/validation**, not independently accepted by this inventory. It does not establish all JSON semantic analyses, source-language parsing, binary analysis, rendering or runtime observation.
- Package metadata/permission inspection and API35+ selected-package lifecycle counters are distinct Android observations. They do not inspect another app's private storage or prove runtime duration, CPU use, traffic or spying.
- Rendered webpage capture, browser interactions and a native browser engine remain open. Static fetched HTML is not a rendered page, screenshot, DOM runtime or browser session.

## Required common process

Each family below must connect to the same normal-request path. A model may help propose an investigation, but Rust owns access, execution, scopes, budgets, state transitions and interpretation of structured observations. Deterministic inspection remains usable without a model.

| Stage | Required native behavior | Required returned evidence and acceptance |
| --- | --- | --- |
| Identify | Resolve the actual subject, user question, accessible inputs, relevant format/domain and current capabilities. | Exact selected identity and scope; ambiguity and unavailable access remain explicit. |
| Discover | Enumerate relevant objects and relationships using bounded readers and supported OS access. | Stable source locations and observed coverage; no guessed objects or silent all-target scanning. |
| Observe | Read or capture the selected subject through the actual engine. | Raw or losslessly addressable observations, time/scope, truncation and error provenance. |
| Explain | Build typed relationships and distinguish measured facts, derivations, hypotheses and unknowns. | Each claim points to supporting observations; unsupported interpretations remain hypotheses. |
| Experiment | Form a finite testable question and perform an authorized bounded read/run/interaction. | Actual inputs, actions, outputs, side effects, deadlines and cleanup status; no model assertion of execution. |
| Compare | Align identities and compare versions, scenarios, traces, source or reconstructed results. | Known differences survive incomplete coverage; missing observations never imply equality. |
| Reconstruct | Produce original implementation from understood requirements and evidence. | Explicit reconstruction obligations and resulting local files; no copied upstream implementation. |
| Verify | Run relevant deterministic checks against captured behavior and the user's required output. | Scope-specific results, remaining unknowns and mismatches; no report-driven production gate. |
| Continue | Preserve context through bounded delegation, cancellation, restart, repair and recursive refinement. | Original task/subject continuity, consumed results, unresolved questions and no side-effect replay. |
| Return | Deliver usable evidence, an answer or a built artifact through native and headless entry points. | Outcome independent of wording or bookkeeping; actual execution failures distinguish transport from semantic failure. |

These stages preserve the public workflow families: feature investigation, version comparison, reconstruction verification, crash investigation, unresolved-question review and controlled process experiments. They are embedded behavior, not an additional user-selected skill.

## Required domain families

`Open` means the family remains implementation work. `Partial` identifies existing narrow behavior only. Every row requires the common process above and original Rust machinery; no row is satisfied by a tool name, schema, placeholder, copied program or configured external endpoint.

The domain names below are a compact coverage index of the published overview; the native work column is RLLM's implementation decomposition. Detailed behavior references follow the table.

| Family | Current native status | Original native work still required |
| --- | --- | --- |
| General artifacts and directories | Partial: bounded file tools and signature/sample inspection. | Typed inventories, content identity, occurrence relationships, reproducible selection and cross-version comparison for all supported inputs. |
| Desktop installations | Open. | Discover installation structure and component roles from supplied files; trace relevant implementation across components without assuming the desktop program can run on Android. |
| Native binaries | Partial: signature/header/string hints. | Complete format readers, instruction decoding, symbols, references, calls, control/data-flow, types, pseudocode and feature traces with precise uncertainty. |
| ELF layout and notes | Partial: basic class/endian/machine hint. | Parse sections, segments, symbols, relocations and notes; validate offsets and declared ranges; separate declared/static mitigation facts from actual runtime protection. |
| PE imports and certificates | Open. | Parse imports, delayed imports, identity/signature structures and exact declared relationships; separate certificate identity, digest consistency and trust validation. |
| Mach-O and Apple bundles | Open. | Parse slices, load commands, linkage, bundle/plist/UI-resource relationships and signature facts; preserve device/simulator and ABI distinctions. |
| Go binaries | Open. | Decode embedded build/module information and relate it to selected binary bytes without requiring a Go installation. |
| Rust binaries | Open. | Decode supported embedded dependency/build metadata and preserve source/edge uncertainty; runtime behavior is not inferred from crate names. |
| GPU code artifacts | Open. | Parse supported GPU containers and intermediate text; expose targets, kernel declarations and resources without claiming execution or acceleration from declarations. |
| Model artifacts | Open. | Bounded format-specific metadata/tensor readers, checked shapes/ranges and identity; loading/inference and actual memory use remain separate engines. |
| IL2CPP metadata | Open. | Versioned metadata readers, image/type/method relationships and cross-file bindings with unsupported revisions explicitly identified. |
| Godot packages | Open. | Format-specific inventory and selected-resource interpretation; integrate safe extraction only through permitted independently accepted archive work. |
| Erlang BEAM | Open. | Decode chunk structure, symbolic inventories, imports/exports and instruction metadata with retained offsets. |
| WebAssembly | Partial: magic-byte classification only. | Original validation, section/type/import/export decoding, instruction semantics and text reconstruction without external tools. |
| EVM bytecode | Open. | Decode instructions and dispatch structures; distinguish exact offsets from inferred signatures, argument interpretation and behavioral hypotheses. |
| SQLite snapshots | Open. | Consistent bounded database/WAL reading, schema and selected data interpretation, without executing target-supplied statements or altering source bytes. |
| Crash recordings | Open. | Parse supported crash/core formats, registers, threads, images and mappings; correlate symptoms and code locations without inventing missing symbols or execution paths. |
| Kernel modules | Open. | Parse module metadata, symbol/version information and signatures; static inspection does not grant kernel access or imply loadability. |
| JavaScript, TypeScript and Electron | Open beyond text inspection. | Original parsers, modules/import/export relationships, source maps, routes, IPC boundaries and semantic graphs; preserve type-only versus runtime relationships. |
| Rendered websites | Open; static HTTP extraction exists. | Original headless HTML/CSS/JavaScript rendering and interaction machinery, DOM/accessibility/script/network capture, fixed or full-page screenshot semantics, sessions, click/type/scroll and user verification handoff. |
| Historical web traffic | Open. | Read supported saved capture formats, exact request/response/payload provenance, selected record paging and safe API-surface derivation without fetching recorded URLs. |
| Managed assemblies | Open. | PE/CLI metadata, types/members/signatures, IL decoding, native boundaries, version comparison and verified reconstruction mapping without loading target assemblies. |
| Android APK code | Partial: package metadata from OS, not APK code analysis. | Original DEX/class/method/reference/decompilation machinery over selected APK bytes; join observed manifest and code evidence. |
| APK signatures | Open as an app capability. | Original signature-block parsing, signer/certificate/rotation relationships and digest verification; development build-tool verification is not this product engine. |
| Android resources | Open beyond OS package fields. | Original binary resource/manifest/string/locale readers and typed relationships to code/UI; no external resource decoder dependency. |
| Android runtime/device observations | Partial: explicit package metadata and lifecycle counts. | Supported permission-scoped process/log/dump/screen/file observations and comparisons, with explicit OS limitations and no required desktop bridge. |
| JEB-style project evidence | Open. | Preserve the class/project/artifact analysis capability through original project readers and native analysis; do not require that external product or its server. |
| Flutter artifacts | Open. | Read selected runtime/snapshot/build identities and relationships; provenance hints must not be called recovered Dart behavior. |
| Firmware | Open. | Original region identification, format interpretation and chained analysis with exact source ranges; archive restriction remains separate and unchanged. |
| Recorded packets | Open. | Original packet/container decoders, fields, filters, reassembly and protocol summaries under bounded state; encrypted content remains unavailable without authorized material. |
| Logic-signal recordings | Open. | Original recording readers, timing/sample interpretation and protocol decoders with exact ranges and explicit ambiguity. |
| Controlled process behavior | Partial: existing reviewed isolated script execution. | Product-owned controlled scenarios, interaction/output/exit/filesystem observations, cancellation/cleanup and comparison; no arbitrary desktop executable support claim. |
| Packaged Python artifacts | Open. | Original container/bytecode metadata readers and source relationships without adding a Python runtime requirement or executing extracted code. |
| Dynamic instrumentation | Open. | Supported on-device observation and controlled probes with explicit authorization and OS boundaries; no external injector/runtime as the native engine. |
| Environment reconstruction and evaluation | Open. | Derive recorded state/transitions/API behavior, build a local runnable model, compare with observations and support bounded task/reward evaluation; no unobserved effects or dependency on external containers. |

A family can contain several formats, versions, processor modes and operation variants. The table is not a license to support just one convenient instance and declare the family complete. Enumerate those variants against public format specifications during implementation. Retain new upstream families and behavior changes in later reconciliation rather than narrowing this baseline.

## Cross-family machinery still required

1. Shared observation identity and typed provenance for raw bytes, source ranges, runtime time ranges, derived relationships and uncertain interpretations. Preserve exact subjects across aliases and transformations; distinguish data unavailable from an observed empty set.
2. Reusable bounded graph and query machinery for references, dependencies, calls, feature routes, source-to-output mapping and evidence-qualified comparison. Page large outputs without pretending an exhausted page is a complete subject.
3. Native discovery and capability reporting that reflects actual loaded readers/engines and supported versions. Optional remote protocol metadata must not advertise native support.
4. Actual reusable analysis state and resumable work with protected drafts/history, explicit cancellation, fresh identities and no implicit replay. Retained results cannot silently become evidence for changed input bytes.
5. Controlled analysis annotations, reconstruction obligations and unknown resolution tied to observations. Missing development reports, manifests or historical acceptance markers never disable an otherwise valid current operation.
6. Process, browser and device observation must expose actual side effects and authorization boundaries. Automatic reasoning is not automatic permission. User-authored examples, target content and returned schemas are data, not executable authority.
7. Native/headless parity through the same Rust implementation and authority checks. A C ABI test exercises a headless route, but does not itself prove installed Android rendering, OS grants, device lifecycle or a complete public headless service.
8. Deterministic multicore scheduling for independent eligible work, checked offsets/lengths, bounded resident memory, optional measured acceleration, operation-local failure and usable non-AI execution.
9. Cross-domain integration with the original compiler/runtime/inference, MCP/ACP/A2A, storage, diagnostics, recursive learning, accessibility and installed-device roadmap. Those requirements are not replaced by investigation work.

## Implementation order and verification

The current JSON/text structure slice must finish its real public inspection path and independent focused verification before being described as delivered. Next select the highest-priority concrete user failure or missing domain path; do not spend successive turns merely refining this inventory. For each family implement the smallest end-to-end useful slice and preserve the rest as active scope.

Required checks for each slice: real supported input; malformed/unsupported input; resource refusal; cancellation/restart; stale callback; raw/evidence identity; ordinary artifact-free execution; isolated failure leaving unrelated workflows usable; same native/headless result; and a separate non-owner review of current bytes. Add actual Android/visual/lifecycle validation where the slice depends on Android behavior. Use independently constructed fixtures and published format facts, not copied upstream implementations or fabricated successful observations.

No archive implementation inspection, archive security review, tests or reproducer was performed for this inventory. The recorded archive restriction remains unchanged. Container-related rows preserve future capability requirements only; they do not authorize retrying or rerouting the rejected review. No complete engine, full REA parity, rendered browser, on-device decompiler, production approval or installed-device acceptance is claimed.

## Public behavior references

The following links identify documentation read for scope, not software to install. No upstream program code is an implementation dependency.

- Overview and domain coverage: https://github.com/morluto/rea (public README, fetched 2026-10-27).
- Process workflows: https://github.com/morluto/rea/blob/main/docs/mcp-prompts.md (guided investigation, comparison, reconstruction, crash and unknown workflows).
- Evidence/session and family contracts: https://github.com/morluto/rea/blob/main/docs/mcp-contracts.md (including generated-catalog availability and family groupings).
- Native investigation: https://github.com/morluto/rea/blob/main/docs/native-investigation.md (functions, instructions, typed references, traces and reconstruction boundaries).
- Application semantics: https://github.com/morluto/rea/blob/main/docs/javascript-application-workflows.md (graph-based feature and version analysis).
- Managed code: https://github.com/morluto/rea/blob/main/docs/managed-code-analysis.md (static members, native boundaries and reconstruction identities).
- Browser observation: https://github.com/morluto/rea/blob/main/docs/browser-observation.md (passive page/script/metadata capture and comparison; RLLM's requested active headless interactions add scope).
- Historical traffic: https://github.com/morluto/rea/blob/main/docs/web-network-captures.md (offline recorded data and retained views).
- Process capture: https://github.com/morluto/rea/blob/main/docs/process-capture.md (finite scenarios, interaction, comparison and finalization).
- Command/evidence/environment behavior: https://github.com/morluto/rea/blob/main/docs/cli.md (shared workflow results, portable evidence and environment comparison).

Documentation identifiers observed through the read-only connector: browser observation `01a940e149489d22cdfc99513aaf4d0d1e9e32ce`; JavaScript workflows `4c746f5d3bcec76b1ef7ba4c994e02d7b73679f4`; managed analysis `6d418aed92338b6c1ef1242d54d899dc48343c49`; historical web captures `b05eb91acda80e2a170c43ec324b5167e6b33085`. These identify the consulted document bytes only and do not control product runtime or certify implementation.
