# Organize Pages

Open a PDF, then choose **Organize pages** in All tools. Click thumbnails, Ctrl+click individual pages, Shift+click a range, or enter a range such as `1-3, 5` and press Select.

Available commands:
- Rotate selected pages clockwise/counterclockwise.
- Delete selected pages with confirmation. At least one page must remain. Undo restores deletion.
- Move one selected page earlier or later. In the source build, enter a final position in **Move to page** and press **Move** or Enter to jump directly there. For example, moving page 1 to position 6 makes it the sixth page; the moved page stays selected. Undo restores the previous order.
- Extract selected pages into one new PDF in document order.
- Undo/redo page operations. A new edit after undo clears the redo branch.
- Save a Copy of the complete working document. Existing files cannot be overwritten. Canceling the file picker leaves edits intact.

Edits remain in memory until saved. Source PDFs are never overwritten. A copy save clears the working document's unsaved indicator; extraction does not. Closing a tab or window with unsaved edits prompts before discarding.

Native output is validated for PDFium readability and page count, written to a temporary file, flushed, and atomically published at a new filename. The original source bytes are frozen at open; memory usage includes those bytes and PDFium's copy in addition to its rendered-page cache.

Limitations:
- Signed/certified/encrypted files are not edited.
- Structural changes to forms, tagged documents, page labels, and article threads are rejected.
- Removing/extracting pages with bookmarks, destinations, names/attachments, open actions, or annotations is rejected until those references can be maintained safely.
- No insert, replace, split, crop, page labels, page boxes, drag-reorder, or one-file-per-extracted-page yet.
- Thumbnail cards are all present in the grid; only nearby thumbnails are rasterized. No measured real-document performance claim.

Verification: seven Rust tests and five frontend tests pass. Coverage includes all 98 synthetic scan pages, 1,500-page fixture loading, duplicate/out-of-range selection, inherited page attributes, order/text preservation, undo/redo branching, protected structures, overwrite refusal, and pixel-identical rotated previews versus saved/reopened files across all valid fixtures. Original fixture bytes remain identical.

Standalone desktop verification: opened the sample, displayed six rendered thumbnails, rotated page 1 to landscape, and saved through the native Save dialog to `artifacts/ui-organized-copy.pdf`. The 6,897-byte output exists and the application reported successful save and cleared its unsaved indicator.
