# Gaps versus the supplied feature reference

## Viewer foundation

No password prompt, PDF text selection, Find, folder search, print, document properties, outlines/bookmark pane, embedded attachments, layer toggles, tags, annotations, signatures, rulers, splits, alternate page layouts, read-aloud, or drag/drop opening yet. Files currently stay open in one native worker. Recent files and stars are session-only. Organizer thumbnails are implemented, but viewer-pane thumbnails, progressive low-resolution previews, cancelable priority queues, crash isolation and auto-save recovery remain unimplemented. Native page edits now export through lopdf.

## Performance and compatibility

No validated 60 fps target, 10,000-page open latency, real 300-dpi scan benchmark, or multi-viewer round trip. The page geometry list is collected at open, rather than a fully lazy page-tree implementation. Frontend page-number lists are not virtualized; rendered page images are. Memory budget applies to the native cache, not a measured total application RSS cap.

## Remaining tools

Organization now supports rotate/delete/extract, move earlier/later, undo/redo and save a new copy. Insert/replace/split/crop/page-label/page-box operations, drag-reordering and extraction into separate files remain unimplemented. Signed or certified documents are rejected for edits; complex page-reference structures are rejected for relevant destructive/structural operations until preservation is implemented. See the tool documentation for exact restrictions.

Creation, combining, text/image editing, format export, OCR, image enhancement, commenting, stamps, measurement, comparison, form preparation, fill/sign, certificate, password security, sanitization, redaction, PDF standards, optimization, accessibility, print preview, action wizard, custom-tool, index and virtual-printer features remain unimplemented. Disabled cards are a roadmap, not evidence of implementation.

## Distribution and verification

No installer/signing/updater, CI, automated desktop UI tests, full dependency-license audit, or 200-file real corpus. Synthetic tests and manual UI checks are recorded separately in phase status. The current build must not be used as a redaction, signing or PDF/A-conformance tool.
