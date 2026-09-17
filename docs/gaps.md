# Gaps versus the supplied feature reference

## Viewer foundation

Embedded-text Find is implemented in the source build: matching pages, highlighted/selectable excerpts, next/previous navigation, case matching, cancellation, and a 500-page result cap. Page text can be selected in a separate dialog. The bookmark pane supports nested entries and internal page destinations, capped at 1,000 entries; bookmark editing and destination zoom/position remain missing. Reading preferences, recent files and stars persist locally. Opening-password prompts support retries and cancellation; encrypted files remain read-only. No on-page text selection or match highlighting, folder search, print, document properties, embedded attachments, layer toggles, tags, annotations, signatures, rulers, splits, alternate page layouts, read-aloud, or drag/drop opening yet. Files currently stay open in one native worker. Organizer thumbnails are implemented, but viewer-pane thumbnails, progressive low-resolution previews, cancelable priority queues, crash isolation and auto-save recovery remain unimplemented. Native page edits export through lopdf. Documents over 65,536 pages are rejected explicitly.

## Performance and compatibility

No validated 60 fps target, 10,000-page open latency, real 300-dpi scan benchmark, or multi-viewer round trip. The page geometry list is collected at open, rather than a fully lazy page-tree implementation. Frontend page-number lists are not virtualized; rendered page images are. Memory budget applies to the native cache, not a measured total application RSS cap.

## Remaining tools

Organization now supports rotate/delete/extract, move earlier/later or directly to a numbered position, undo/redo and save a new copy. Insert/replace/split/crop/page-label/page-box operations, drag-reordering and extraction into separate files remain unimplemented. Signed or certified documents are rejected for edits; complex page-reference structures are rejected for relevant destructive/structural operations until preservation is implemented. See the tool documentation for exact restrictions.

Creation, combining, text/image editing, format export, OCR, image enhancement, commenting, stamps, measurement, comparison, form preparation, fill/sign, certificate, password security, sanitization, redaction, PDF standards, optimization, accessibility, print preview, action wizard, custom-tool, index and virtual-printer features remain unimplemented. Disabled cards are a roadmap, not evidence of implementation.

## Distribution and verification

The installer, signed updater, and release CI are implemented. Azure credentials are configured; a publisher-signed release and full upgrade/relaunch still need verification. Automated desktop UI tests, a full dependency-license audit, and a 200-file real corpus remain outstanding. Synthetic tests and manual UI checks are recorded separately in phase status. The current build must not be used as a redaction, document-signing or PDF/A-conformance tool.
