# Organize Pages

Open a PDF, then choose **Organize pages** in All tools. Click thumbnails, Ctrl+click individual pages, Shift+click a range, or enter a range such as `1-3, 5` and press Select.

Available commands:
- Rotate selected pages clockwise/counterclockwise.
- Delete selected pages with confirmation. At least one page must remain. Undo restores deletion.
- Move one selected page earlier or later. In the source build, enter a final position in **Move to page** and press **Move** or Enter to jump directly there. For example, moving page 1 to position 6 makes it the sixth page; the moved page stays selected. Undo restores the previous order.
- Extract selected pages into one new PDF in document order.
- Crop one selected page. Organize Pages starts on the page you were viewing; enter finite, non-negative top, right, bottom, and left insets in points. The preview shows the resulting page size, and each dimension must retain at least 1 point. Cropping hides content and is not redaction.
- Split the current edited page order into fixed-size files. Enter a positive whole number of pages per file; the preview limits the operation to 64 output files. Windows asks for a new output folder, and canceling that dialog leaves the document unchanged.
- Undo/redo page operations. A new edit after undo clears the redo branch.
- Save a Copy of the complete working document. Existing files cannot be overwritten. Canceling the file picker leaves edits intact.

Edits remain in memory until saved. Source PDFs are never overwritten. A copy save clears the working document's unsaved indicator; extraction does not. Closing a tab or window with unsaved edits prompts before discarding.

Native extraction output is validated for PDFium readability and page count, written to a temporary file, flushed, and atomically published at a new filename. Split output validates every file in a staged folder before atomically publishing that new folder; existing folders are never replaced. The original source bytes are frozen at open; splitting uses the current edited page order but does not clear the unsaved indicator, alter the save baseline, or change undo/redo history. The 64-output limit and 256 MiB limit per split file protect output creation; neither is an application memory cap.

Limitations:
- Signed/certified/encrypted files are not edited.
- Structural changes to forms, tagged documents, page labels, and article threads are rejected.
- Removing/extracting pages with bookmarks, destinations, names/attachments, open actions, or annotations is rejected until those references can be maintained safely.
- No insert, replace, page labels, page boxes, drag-reorder, or one-file-per-extracted-page yet.
- Thumbnail cards are all present in the grid; only nearby thumbnails are rasterized. No measured real-document performance claim.

Verification: 52 native tests and 87 frontend/release tests pass, along with the production frontend build and standalone Windows debug build. Split coverage includes current edited pages across all three valid fixtures, output grouping, validation rollback, existing/racing-folder refusal, source/session preservation, invalid UI inputs, canceled native folder selection, retry, duplicate submission, busy-state guarding, and unchanged document state. Crop coverage includes current-page initialization, rotated displayed dimensions, finite/non-negative and one-point boundaries, stale/error/cancel/retry behavior, duplicate guarding, pixel-preserving saved output, and unchanged source bytes. A browser harness verified crop preview and the normalized request with mocked IPC; it does not verify native desktop interaction. Original fixture bytes remain identical. Export, validation, and publication for three outputs took 15.6 ms, 305.2 ms, and 631.6 ms for 5, 97, and 1,499 edited pages respectively; these are split fixture timings, not a whole-application benchmark.

Standalone desktop verification: opened the sample, displayed six rendered thumbnails, rotated page 1 to landscape, and saved through the native Save dialog to `artifacts/ui-organized-copy.pdf`. The 6,897-byte output exists and the application reported successful save and cleared its unsaved indicator.

Native folder-dialog interaction for splitting and native crop interaction remain unverified in the desktop application.
