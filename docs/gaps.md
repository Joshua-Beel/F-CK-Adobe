# Gaps versus the supplied feature reference

## Viewer foundation

No password prompt, PDF text selection, Find, folder search, print, document properties, outlines/bookmark pane, embedded attachments, layer toggles, tags, annotations, signatures, rulers, splits, alternate page layouts, read-aloud, or drag/drop opening yet. Files currently stay open in one native worker. Recent files and stars are session-only. Thumbnail images, progressive low-resolution previews, cancelable priority queues, crash isolation and auto-save recovery are not yet implemented. There is no mutation path.

## Performance and compatibility

No validated 60 fps target, 10,000-page open latency, real 300-dpi scan benchmark, or multi-viewer round trip. The page geometry list is collected at open, rather than a fully lazy page-tree implementation. Frontend page-number lists are not virtualized; rendered page images are. Memory budget applies to the native cache, not a measured total application RSS cap.

## Remaining tools

All creation, assembly, organization, editing, export, OCR, image enhancement, commenting, stamps, measurement, comparison, form preparation, fill/sign, certificate, password security, sanitization, redaction, PDF standards, optimization, accessibility, print preview, action wizard, custom-tool, index and virtual-printer features remain unimplemented. Disabled cards are a roadmap, not evidence of implementation.

## Distribution and verification

No installer/signing/updater, CI, automated desktop UI tests, full dependency-license audit, or 200-file real corpus. Synthetic tests and manual UI checks are recorded separately in phase status. The current build must not be used as a redaction, signing or PDF/A-conformance tool.
