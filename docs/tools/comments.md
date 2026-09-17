# Comments

Open a PDF and choose **Comment** from All tools or the quick toolbar. Click a page to place one sticky note, type its text, then choose **Save comment**. The PDF renders the note icon; the transparent on-page target opens the editor without drawing a second icon.

Use the **Comments** rail button to list every note. Select a note to edit or delete it. A note fully hidden by the current crop has no on-page target, but remains in the list with an explicit hidden status and can still be edited or deleted. Press **Escape**, choose Select, or press **H** to leave placement mode. While a note is saving, closing the editor, tab, or workspace is blocked.

Notes have no author field in this slice. A note must contain nonblank, NUL-free text and may use at most 8 KiB of UTF-8; a document may have at most 1,000 notes and 1 MiB of note text. Current page crop and rotation are handled by the native layer, so the UI always sends a normalized position in the displayed page rather than converting PDF coordinates itself.

Supported notes follow page rotation, crop, move, delete, extract, and split operations. They participate in undo/redo, Save a Copy, reopening, visible rendering, and immutable print snapshots. Printed pages render the note icon only; note bodies do not enter Page text or Find.

Commenting is unavailable for foreign, empty, malformed, signed, encrypted, form, tagged, page-label, or article-thread annotation structures. This refusal leaves the source unchanged. Combine Files, Insert Pages, and Replace Pages also refuse annotated source documents. Markup types beyond sticky notes, replies, author identity, and foreign-annotation interchange are not implemented.

Verification includes 123 frontend/release tests and 94 native tests with one ignored manual probe; the production frontend and standalone Windows debug builds passed. Nine focused native comment regressions cover cardinal rotations, inherited and offset crops, Unicode, history limits, reopening, marker pixels, immutable print snapshots, supported page operations, and refusal cases. Focused UI tests and a mocked browser harness cover placement, transparent hit geometry, hidden-note editing, stale requests, UTF-8 limits, and busy/Escape behavior. Desktop native-dialog, save, print, and installed-app checks remain separate evidence.
