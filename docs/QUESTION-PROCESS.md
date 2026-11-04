# Question and design process

RLLM's shared Rust process supports research, troubleshooting, original design and coding. User requests create workspace-bound questions; recursive and delegated tasks retain their parent relationships. Plans, evidence, interpretations, checks and answers persist in the application journal.

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
