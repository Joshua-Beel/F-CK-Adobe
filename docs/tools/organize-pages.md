# Organize Pages

Open a PDF, then choose **Organize pages** in All tools. Click thumbnails, Ctrl+click individual pages, Shift+click a range, or enter a range such as `1-3, 5` and press Select.

**Combine Files** is a separate All tools command. With two distinct PDFs open, choose the first and second document; it writes a new copy containing their current edited pages in that order. The first PDF’s Info/XMP metadata is retained without merging or inventing metadata. Both source tabs, including unsaved edits and history, stay unchanged.

**Insert pages** is available from Organize Pages. Choose distinct target and donor PDFs, then a boundary from 0 before the target’s first page through its page count after the last page. The new copy contains the target prefix, every current edited donor page, then the target suffix. The target PDF’s Info/XMP metadata is retained; both source tabs stay unchanged.

**Replace pages** is available from Organize Pages. Select one contiguous target range, choose a distinct donor PDF, and save a new copy. The new copy contains the target prefix, every current edited donor page, then the target suffix. The target PDF’s Info/XMP metadata is retained even when the selected range is the whole target; both source tabs stay unchanged.

Available commands:
- Rotate selected pages clockwise/counterclockwise.
- Delete selected pages with confirmation. At least one page must remain. Undo restores deletion.
- Move one selected page earlier or later. In the source build, enter a final position in **Move to page** and press **Move** or Enter to jump directly there. For example, moving page 1 to position 6 makes it the sixth page; the moved page stays selected. Undo restores the previous order.
- Extract selected pages into one new PDF in document order.
- Crop one selected page. Organize Pages starts on the page you were viewing; enter finite, non-negative top, right, bottom, and left insets in points. The preview shows the resulting page size, and each dimension must retain at least 1 point. Cropping hides content and is not redaction.
- Split the current edited page order into fixed-size files. Enter a positive whole number of pages per file; the preview limits the operation to 64 output files. Windows asks for a new output folder, and canceling that dialog leaves the document unchanged.
- Insert every current edited page from a distinct open donor PDF into a new target copy. Choose the insertion boundary before the first page, between pages, or after the last page. Windows asks where to save the new PDF; canceling leaves both source documents unchanged.
- Replace one contiguous target range with every current edited page from a distinct open donor PDF. The preview shows the selected range and resulting page count. Windows asks where to save the new PDF; canceling leaves both source documents unchanged.
- Undo/redo page operations. A new edit after undo clears the redo branch.
- Save a Copy of the complete working document. Existing files cannot be overwritten. Canceling the file picker leaves edits intact.

Edits remain in memory until saved. Source PDFs are never overwritten. A copy save clears the working document's unsaved indicator; extraction does not. Closing a tab or window with unsaved edits prompts before discarding.

Native extraction output is validated for PDFium readability and page count, written to a temporary file, flushed, and atomically published at a new filename. Split output validates every file in a staged folder before atomically publishing that new folder; existing folders are never replaced. Combine, Insert Pages, and Replace Pages validate both current source plans and every new output before publishing a new file without replacement; they reject unsupported catalog/page-tree features, cap aggregate input at 4,096 pages and serialized output at 256 MiB, retain the first/target PDF’s Info/XMP metadata, and leave both source sessions unchanged. These limits protect output creation; they are not application memory caps. The original source bytes are frozen at open; splitting uses the current edited page order but does not clear the unsaved indicator, alter the save baseline, or change undo/redo history.

Limitations:
- Signed/certified/encrypted files are not edited.
- Structural changes to forms, tagged documents, page labels, and article threads are rejected.
- Removing/extracting pages with bookmarks, destinations, names/attachments, open actions, or annotations is rejected until those references can be maintained safely.
- No page labels, page boxes, drag-reorder, or one-file-per-extracted-page yet.
- Thumbnail cards are all present in the grid; only nearby thumbnails are rasterized. No measured real-document performance claim.

Verification: the replacement source gate passed 109 frontend/release tests, 85 native tests with one separately ignored manual probe, the production frontend build, and a standalone Windows debug build. Replacement coverage includes contiguous ranges, shorter/longer donors, whole-target replacement with target metadata, stale/closed/duplicate guards, current-source preservation, output validation, and unchanged source history. A mocked-IPC browser harness checked the selected-range preview, result count, and submitted revisions; it does not verify the native save dialog. A prior interrupted native run had no test-result footer; its cause remains unconfirmed and the captured rerun passed. Earlier Combine and Insert Pages coverage includes all six ordered pairs of the three document fixtures, every output page, cropped rotation ink/tokens, first/target metadata, stale/closed/duplicate guards, unsupported-structure refusal, source/history preservation, output validation, and existing/racing-file refusal. Insert Pages also checks boundaries at the beginning, middle, and end. Split coverage includes current edited pages across all three valid fixtures, output grouping, validation rollback, existing/racing-folder refusal, source/session preservation, invalid UI inputs, canceled native folder selection, retry, duplicate submission, busy-state guarding, and unchanged document state. Crop coverage includes current-page initialization, rotated displayed dimensions, finite/non-negative and one-point boundaries, stale/error/cancel/retry behavior, duplicate guarding, pixel-preserving saved output, and unchanged source bytes. Original fixture bytes remain identical. Export, validation, and publication for three outputs took 15.6 ms, 305.2 ms, and 631.6 ms for 5, 97, and 1,499 edited pages respectively; these are split fixture timings, not a whole-application benchmark.

Standalone desktop verification: opened the sample, displayed six rendered thumbnails, rotated page 1 to landscape, and saved through the native Save dialog to `artifacts/ui-organized-copy.pdf`. The 6,897-byte output exists and the application reported successful save and cleared its unsaved indicator.

Native folder-dialog interaction for splitting, native crop interaction, and native save dialogs for Combine Files, Insert Pages, and Replace Pages remain unverified in the desktop application.
