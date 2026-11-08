# Question and design process

RLLM's shared Rust process supports research, troubleshooting, original design and coding. User requests create workspace-bound questions; recursive and delegated tasks retain their parent relationships. Plans, evidence, interpretations, checks and answers persist in the application journal.

## Start from conversation

You do not have to fill in the editable investigation, hypothesis or plan forms. Tell the assistant what to investigate, which sources it may use, the behavior you want to reproduce, and any implementation language or constraints. For example: “Investigate this program's observable behavior. Record the findings and unresolved questions, then produce a behavioral specification and original implementation plan in Rust. Do not copy its implementation.” The same request can name another language or ask for a language-neutral plan.

Every ordinary request starts its question automatically. The shared model guidance describes the investigation process even when additional investigation preferences are off. If the selected agent permits `hypotheses`, it can record the actual plan, findings and assessments through reviewed operations. The default General agent includes that operation. A restricted agent may still answer in chat; it cannot silently acquire a disabled operation. File, web, delegated and write operations retain the current permissions and approvals. The app does not force every model to use a tool or guarantee a complete plan from a prose-only response.

The editable forms are optional manual contributions to the same records. Additional investigation preferences customize guidance; they are not a requirement to start. Model-generated plans and findings appear in Question and design process after their operations are adopted.

## Design brief

Choose Process export → Design brief (Markdown) to export the recorded goal, questions, requirements, constraints, findings, unresolved work and evidence/check references. The complete journal JSON remains the default export. Both formats require the same current-question review and new-file write approval. Changed content, changed format, cancellation, source-policy changes and stale execution cannot borrow another approved export.

Permitted model operations can request `hypotheses({process:{action:"brief"}})` to read the same brief. It does not select a language or run a compiler; a requested language is retained as an authored plan constraint. Missing plans, findings or evidence remain explicitly unrecorded or unresolved. Returned-text comparisons remain distinct from authored assessments and do not certify a complete implementation.

The brief omits raw returned source/output bodies, request arguments, prior raw answers and expected source text from finite comparisons. It preserves references and the measured coverage of returned text. User/model-authored entries may themselves quote source material. A brief does not establish separate analysis and implementation contexts; review its actual contents before using it as an original implementation specification.

## Manual workflow

Open Investigation → Question and design process. Create or select a question, edit its plan, add findings/design/unknown notes, assess planned items, review finite checks and export the journal. Choose Use for manual observations to associate subsequent operations with that question. Manual controls use the same process engine as ordinary and recursive tasks.

Exports use the reviewed new-workspace-file operation. Source authority is checked at review, approval and execution.

## Evidence and interpretation

Owned file, web and other operation completions create evidence receipts bound to their operation, workspace and current rules. Receipts retain returned text, lengths, hashes, source arguments and truncation metadata. Authored hypotheses, inferences, designs, resolutions and returned answers have separate record types.

Finite checks support `contains`, exact returned-text equality and SHA256 of actual returned text. Each verdict follows the retained output and its coverage. Prefix matches support observed substrings; incomplete output keeps unmeasured absence/equality questions Unresolved. Views and exports recompute check verdicts from retained data.

## Plans and assessments

Plans contain questions, requirements and constraints. Planned item review lists each item and its current authored assessment. Assessments record answered, partial or unresolved, with an explanation and optional actual receipt/check references. Each assessment binds to the exact plan revision and item text. Editing a plan retains earlier assessments as history and gives revised items their current review state.

## Model operations

Permitted `hypotheses` calls accept a `process` object. Actions include `current`, `plan`, `note`, `check`, `assess`, `review` and `list`. Section views include plan, review, assessments, evidence, notes, checks and answer, with paged offsets. Tools retain current user rules, permissions and approvals.

## Journal capacity

The journal supports32questions,512KiB total and96KiB per question. Each question retains up to16outputs and32notes/checks; each output retains up to8192UTF-8 bytes. Views paginate and label previews, while export contains the canonical retained records. Capacity and retention settings preserve ordinary operation availability and existing stored records.

## Deleting a question

Deletion displays the reviewed question and ID. Its confirmation binds to that exact journal content. Switching questions, editing the journal or leaving review invalidates the captured confirmation. A fresh confirmation deletes only the reviewed journal and clears its active pointer; conversations, source files, observation records and exports remain.

Provider search results automatically retain their reported query/title/URL/error metadata with measured byte counts and digests. Native views, finite comparisons, refinement and reviewed exports use those same records.
