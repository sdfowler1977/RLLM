# Chat reading and reply display

Incoming replies preserve your reading position. When already at the bottom, the chat follows the bottom of the growing reply, including a reply taller than the screen. Scroll up to read earlier text without being pulled back. Tap **Latest ↓** to jump to the current end and resume following it. Keyboard and viewport size changes preserve the selected reading/follow behavior.

Explicit leading `<think>...</think>` model blocks are hidden from normal chat and History previews, including incomplete streamed blocks. While only such reasoning has arrived the chat shows “Thinking…”. If a completed response contains no final answer, it shows “No final answer returned”. Raw saved model output is retained. Quoted, fenced and inline examples, user messages and unmarked prose remain unchanged; the app does not guess whether ordinary prose is reasoning. Separate structured provider reasoning fields remain excluded from visible answers.

This is display behavior. It does not execute tool JSON hidden inside reasoning or relax the whole-response bare-JSON proposal contract. Raw message inspection remains separate. No user conversations are deleted by this update.
