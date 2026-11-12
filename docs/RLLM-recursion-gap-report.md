# RLLM Recursion and Reverse-Engineering Gap Report

**From:** Bar (completeness review)
**For:** the agent working on RLLM (`/home/shawn/RLLM` on bodhi)
**Date:** 2026-11-11
**Verdict:** BAR-MISS
**Evidence:** static. Every finding comes from reading the current RLLM source tree, not from running it. The tree has no git metadata, so findings are against the files as they are now.

---

## 1. The yardstick: review your work against these

Bob's rule: RLLM must have every recursive feature from these sources built in as core features. They must be white-room rebuilt in native Rust. Never download, install, vendor, or call the upstream code. Calling outside services as tools, such as web search, is fine.

- **RRSI: Regularized Recursive Self-Improvement of Agent Harnesses**
  - Paper: https://arxiv.org/abs/2609.24972
  - Code: https://github.com/google-research/rrsi
- **Harness²: Recursive Agent Harnessing for an Open World**
  - Code: https://github.com/google-research/harness2
  - Paper: https://arxiv.org/abs/2610.14224
- **REA: Reverse Engineer Anything**
  - Code: https://github.com/morluto/rea
  - It must be built into RLLM as a core tool, not run as an external tool or MCP server.

**How a feature gets added:** when asked to figure something out or add a feature, RLLM researches how it works elsewhere and what else uses it. It inspects real evidence, writes a clean-room spec, then builds a white-room implementation.

**Already settled; do not reopen:**
- There is no turn or round cap. Loops run until Bob stops them.
- YOLO mode never asks.
- Web search and fetch are always approved in every mode.
- Shell, git, and hosted web search are fine as tools.

---

## 2. What RLLM already has (keep it)

- **A propose → probe → compose → verify refine loop** (`core/harness.rs` ~209, 396–520).
  - A lessons store (`core/harness.rs:226-232, 1054-1065`).
  - A `verified_improvement` check before a profile is saved (`core/improvement.rs:304,357`).
- **Strong investigation tools** (`core/investigation.rs`): inspect, compare, observations, hypotheses, and experiments.
- **Reviewed `reconstruct` with SHA-256 readback** (`core/reconstruction.rs:1,22-23,29`).
- **Question and design journal** (`core/inquiry.rs`, `core/design_brief.rs`). It never gates ordinary work.
- **Delegation stack** (`core/delegation.rs`). Rejecting a child stops that child.
- **Permission classes** (`core/permissions.rs`). Web stands down in every mode.
- **No copy of rrsi, harness2, or rea** was found in the tree. Good.

---

## 3. Ranked gaps

### Gap 1: Rebuild and reload itself (missing)

**What's thin:** RLLM can edit, build, and test in principle. But nothing rebuilds RLLM and then switches the running process to the new build. Nothing in `core/` drives `package.sh` and then re-execs.

**What finished looks like:** RLLM does all of this inside one job:
1. edits its own source;
2. builds and tests it;
3. commits it;
4. re-execs, or starts a successor session on the new binary;
5. carries on with the same job.

A real run of that sequence must be on file.

**Why good enough fails:** Bob's rule is that the program is recursive. Improvements that never get loaded do not count as self-improvement.

### Gap 2: Full RRSI loop (partial)

**What's thin:**
- Refine rewrites agent instructions only (`core/harness.rs:402-434`).
- It stops after two rounds (`core/improvement.rs:109-117`).
- It scores against user phrase/JSON checks only (`core/improvement.rs:42-70`).
- It keeps or drops on raw check counts (`improvement.rs:121-137`, `harness.rs:456-497`).
- It evaluates one candidate at a time.

**What finished looks like:** each RRSI feature, built natively.

| # | RRSI feature | Finished means |
|---|---|---|
| 1 | Harness-level self-improvement with frozen model weights | A multi-round loop that edits the harness, not the model |
| 2 | Open edit space | Prompts, control flow, config, context management, tools, skills, memory, and subagents are all editable |
| 3 | Annealed edit budget | A cap on independently attributable edits per candidate. The cap shrinks from b_max to b_min over the rounds |
| 4 | Evidence-aware edit history | Each edit records its component, hypothesis, diff, score and cost change, and whether it was accepted or rejected. Later proposals read this history, and falsified ideas are not proposed again |
| 5 | Stall exploration | If progress over w rounds stays inside the noise band δ, slots are reserved for components not yet tried |
| 6 | Leakage critic before scoring | A candidate that encodes test-suite task names or answers, suite-specific logic, or inert machinery is rejected before it can get an inflated score |
| 7 | Noise-adjusted floor | δ is estimated from repeated evaluations of the base harness. Acceptance needs Ŝ(H′) ≥ S* − δ |
| 8 | Cost-aware acceptance | ΔC ≤ β0 + β1·ΔS. Within the noise band, only a cost saving or structural novelty is accepted |
| 9 | Structural pruning | A component with no strictly positive gain over a window is deleted |
| 10 | Evolve / held-out / OOD split | The same unchanged harness is scored on a held-out set and on a different benchmark |
| 11 | Parallel candidate evaluation | If no candidate is admissible, the incumbent is kept |
| 12 | Git worktree candidates | Each candidate lives in its own worktree. Acceptance fast-forwards an evolve branch, so the incumbent is always a commit |
| 13 | Auditable, resumable run | JSONL history, proposer/analyst/critic prompts as plain files, a STOP file, resume, and re-judging from stored evidence |

**Why good enough fails:** a two-round instruction rewrite scored against fixed phrases can overfit and pile on cost. RRSI's safeguards exist to stop exactly that.

### Gap 3: Harness² two-level recursion (partial)

**What's thin:** RLLM has one instruction loop shaped like PPC, plus a few lessons marked as unverified. Missing:
- composition from behaviour contrasts without an answer key;
- parallel candidates;
- a persistent improvement library that later tasks actually read.

**What finished looks like:**
1. **Task level:** contrastive Propose–Probe–Compose. Edits are probed on the same task, and the successor is composed from the behaviour contrasts it observed, not from an assumed score.
2. **Parallel recursion:** k invariant candidates (how the agent works) plus k domain-specific candidates, then k compositions drawn from the whole pool.
3. **Sequential recursion:** k chained PPC steps that carry the refined harness and its evidence forward.
4. **Harnessing level:** after each task, a persistent improvement library is updated with the adopted harness and the lessons the evidence supports. This is EXPERIENCE.md plus an index. Task t+1 starts from the base harness but is actually guided by that library.
5. **The eight editable components:** system prompt, guardrails, memory, tool guidance, subagents, skills, scripts, and plugins.
6. **Invariant vs domain scoping** for every proposal.
7. **Agentic selector:** when k>1, choose among anonymised deliverables.
8. **Resume** with the retained summaries, deliverables, adopted harnesses, and reflection notes.

**Why good enough fails:** without the library, each task starts over. Bob wants lessons that build up over time.

### Gap 4: REA as a core tool, with white-room feature rebuild (partial)

**What's thin:**
- `reconstruct` either copies observed bytes, exports a retained representation, or writes a structural spec. It explicitly does not rebuild behaviour.
- Native analysis is bounded: ELF/PE observations, AArch64 mapping symbols, candidate blocks, and a local IR (`elf_code.rs:16-18`, `harness.rs:306`). There is no decompiler-grade pseudocode.
- These are missing entirely: JS/Electron analysis, .NET/CIL, and analysis of saved HAR/SAZ traffic.

**What finished looks like:**
1. **Feature rebuild:** "how does this feature work, show evidence, build a clean-room version" runs end to end. Research, inspect, a cited evidence report, a clean-room spec, the implementation, and verification against the reference. It never copies third-party source.
2. **Native binaries:** pseudocode, assembly, strings, symbols, calls, and cross-references.
3. **JavaScript / Electron:** modules, imports, source maps, routes, IPC, native add-ons, and ASAR.
4. **.NET:** metadata, CIL, native dependencies, and build comparison.
5. **Web:** page structure, scripts, network observations, and screenshots. Much of this already exists.
6. **Captures:** saved HAR/SAZ files and process/runtime capture.
7. **Evidence contract:** every finding says what is known and what is unknown. This is near present for file and web observations.
8. **Desktop / package survey:** installs, supporting files, and next probes.
9. **Broader formats** from REA (firmware, packets, APK). RLLM keeps its investigation path for these.

**Why good enough fails:** without a behaviour rebuild, RLLM can describe a feature but cannot add it. Bob's whole feature-add path depends on that.

### Gap 5: Python in RLLM's own tooling (R-rule)

**What's thin:** these files are Python: `tools/apk.py`, `tools/package.py`, `tools/checks.py`, `tools/alignment.py`, `tools/checkpoint.py`, `tools/readback.py`, `tools/signature.py`, `tools/toolchain.py`.

**What finished looks like:** packaging and verification are native, like the runtime. A bridge is allowed only where no native route exists at all, and the work must state why.

**Why good enough fails:** Bob's rule is that none of his programs run on Python.

---

## 4. Proof needed before a re-review

- A real run log for each gap, made on the committed bytes. Source alone is not enough, and logs made before the commit do not count.
- **Gap 1:** a log showing edit → build → test → commit → re-exec → continuing the same job.
- **Gap 2:** an RRSI run of at least three rounds showing:
  - the budget shrinking;
  - at least one rejected leaking candidate;
  - a noise-floor decision;
  - a pruned component;
  - the incumbent as a worktree commit;
  - a held-out score.
- **Gap 3:** task t+1 visibly reading the library entry written by task t.
- **Gap 4:** one white-room feature rebuild from a reference, with its evidence report and verification.
- No stubs and no fake passes. A failure must show as a failure.
