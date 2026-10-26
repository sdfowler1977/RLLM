# Use RLLM

## Chat controls

The top pencil edits the current connection. The footer's left control selects a saved connection or adds another one. The centered control selects the main-agent profile. The quiet number on the right opens analytics. Changing the connection preserves the thread; the next request uses the selected endpoint/model. Earlier replies retain their original attribution. Finish or cancel the active request before changing its routing.

The plus beside the composer opens conversation actions. On very constrained keyboard layouts, the compact connection menu also provides access to conversation actions and the main-agent picker. The navigation menu keeps **Chat**, **History**, **Workspace**, **App settings**, and **Agent settings** separate.

## History and deletion

Open **History** to return to an earlier conversation. Its options control exposes that conversation's actions. **Delete conversation** asks for confirmation and moves the selected chat to **Trash**. Trash can restore the chat or permanently delete that specific item after another confirmation. Closing a confirmation makes no deletion. An active chat's saved draft stays with the trashed conversation.

Create a new conversation when you want independent context. Switching an LLM is not the same as creating a new conversation.

## Appearance and accessibility

Use **App settings → Appearance** for dark/light theme, waves/solid/custom-image background, font family, text scale, and custom text/surface/accent colors. Use **Preview** before saving. **Reset appearance** returns to defaults. Android system text scaling also applies. The supplied interfaces use proportional text; message cards grow with wrapped text.

Custom color combinations can reduce contrast. Choose readable combinations and inspect them at your preferred text size. The APK's current large-font, TalkBack, and keyboard behavior still needs physical-device verification.

## Agents, skills and tools

Give a profile a concrete purpose and a recognizable name, such as **General**, **Planner**, or **Research**. Instructions describe the job; skills supply reusable procedures. Tool access is a separate permission list. A skill cannot grant permissions that the profile lacks.

Example planning instructions:

```text
Clarify the goal, identify missing information, propose practical steps,
and verify results against available evidence. Prefer a read-only tool
when it is sufficient. Explain uncertainty and ask before changing files.
```

Choose callable subagents in the profile editor. Current reviewed nested delegation is bounded to depth three and eight reviewed operations per task. Each child receives its captured profile and assigned connection. Child results return as evidence for the parent; they are not automatically trusted commands.

Review proposed file/network/tool operations before approving them. Cancellation should end the current operation, not erase completed work. A remote request already transmitted may still be running on the server.

## Context and compaction

Set the default compaction connection in **Agent settings → Default models**. Use the conversation's agent-task/compaction actions when you decide to compact. Keep an export of important conversations before experimenting with summaries. Model-generated summaries can omit details; they are not an exact archival copy.

## Refinement

The task refinement workflow captures a baseline, proposes a candidate profile, runs a bounded read-only probe, composes a result, and lets you explicitly adopt the new profile. The original profile and global tool restrictions remain protected. Saved refinement records are inspectable, but this build does not automatically feed all saved lessons into later tasks or train the underlying model.

## Workspace and exports

Workspace tools operate on project files; phone and remote-storage tools are separately configured and reviewed. Export a conversation/configuration through its corresponding action, or use configured supported storage. Keep original files when an operation reports a partial failure. Current project ZIP bounds are 500 entries, 1 MiB per file and 4 MiB of file data; larger individual-file transfers use a separate bounded transfer path.

Project archive import is still a test-build feature with an outstanding independent repair review. Use a disposable workspace for that testing and keep important files backed up. Do not infer archive acceptance from the successful chat/analytics checks.
