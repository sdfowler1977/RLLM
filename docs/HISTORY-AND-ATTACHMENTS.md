# History and phone attachments

Open History from the header. Each card shows the conversation title, a short latest-message or draft preview, message count, and a direct Delete button. Delete confirms the exact conversation and moves it to Trash. Deleting the final conversation leaves History empty; restart does not create a replacement. Restore from Trash preserves the conversation and its draft. Starting or sending a new conversation creates a new record.

In Chat, tap **+ → Attach image or text file**. Choose a PNG/JPEG image or UTF-8 text file using Android's file picker. Images are listed above the composer with their filename and dimensions; Remove detaches an unsent image. Text file content appends to existing draft text. Nothing is sent just by selecting a file. Press Send to share the draft and attached images with the selected model endpoint.

The app sends actual image bytes using each provider's image request format: Chat image_url data URI, Messages base64 image source, Content inlineData, and local chat images. Your configured model must support images. Unsupported model responses remain visible; the app does not silently convert an image into filename-only text.

PNG/JPEG files are limited to 2 MiB each and 2 MiB total across up to four images in the outgoing context. Text documents and composed text retain the 1 MiB limit. Request, memory and encrypted-history limits remain unchanged; refusal preserves the existing draft. PDF/Office document extraction and other image formats are not implemented by this change.

Unsent images belong to the exact conversation. Navigation, replacement picker requests, cancellation, restart, busy work, changed agent or changed connection prevent a stale picker result from being adopted. Images survive saved drafts, chat switching, Trash restoration and accepted sends. Image bytes remain in encrypted state; the UI receives filename/dimensions, not duplicated base64 payloads.

Typed image context follows explicit refinement/delegation snapshots and compaction. It is not pasted into model text as base64 or automatically attached to unrelated explicit tasks. Context byte limits include text plus encoded image bytes; these are not exact image token counts.
