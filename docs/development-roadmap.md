# Remaining development and verification

The README records what is implemented and tested. This plan organizes the remaining work; a completed source feature does not establish that the installed application works. The original brief and `gaps.md` remain the broader feature reference.

## Team rules

- GPT-6 handles planning, ownership, dependencies and acceptance decisions.
- GPT-5.6 Sol handles complex native PDF work and adversarial review; GPT-5.6 Terra handles UI and routine integration; GPT-5.6 Luna handles bounded exploration when a slot is available.
- Each implementation has explicit file ownership and a defined interface. Agents coordinate shared contracts before editing consumers.
- Only one native test/build process runs at a time. Combined tests and applicable artifact checks must pass before explicit-path commits and pushes.
- Each feature receives an independent review where capacity permits. Findings need evidence and regression coverage; unsupported or unverified behavior stays documented.

## Work sequence

| Slice | Dependencies | Acceptance evidence |
| --- | --- | --- |
| Search highlighting | On-page text selection source slice | Match locations map to rendered pages without obscuring selection; stale results, page edits, crop offsets and rotations behave explicitly. |
| Installed printing and update verification | Existing print implementation and a current signed installer | Inspect Print to PDF output and printer cancellation; install and upgrade from 0.2.0, preserve documents/settings, relaunch and exercise interruption/retry. The existing desktop-control blocker must change before repeating attempts. |
| Create/combine/insert/replace pages | Preservation rules and an expanded document model | Reopened output retains intended order, page content and inherited attributes; originals stay byte-identical; unsupported forms, signatures and references are rejected before mutation. |
| Split/crop/page labels and page boxes | Page operations and undo/save model | Exact ranges and boxes survive reopening; undo/redo and save branches remain correct; rotated and mixed-size documents are covered. |
| Comments, markup and annotation persistence | Page geometry and preservation rules | Create/select/edit/delete supported annotations, reopen saved copies and compare positions/content; existing annotations survive unrelated supported edits. |
| Fill existing forms, then form preparation | Form-aware preservation and save validation | Fill supported fields and reopen in another viewer; appearances, values and flags agree; unsupported field/signature behavior is explicit. |
| Text and image editing | Font/image handling and content-preserving export | Changed content, appearance and undo survive reopening; unsupported fonts/layouts cannot silently corrupt or substitute content. |
| OCR and conversion | Engine choice, dependency/license review and document model | Scans produce aligned searchable text; representative layouts and languages have measured accuracy; conversion losses are disclosed and originals preserved. |
| Redaction and sanitization | Content editing and extraction-aware verification | Removed content cannot be recovered through text, objects, images, attachments or metadata; reopen/render and adversarial extraction checks prove removal. Visual masking alone never qualifies. |
| Resource limits, isolation and recovery | Worker lifecycle and persistence design | Measure large-file and rapid-scroll workloads; bound queued/retained work; recover from worker failure without losing acknowledged edits; test corrupt recovery data. |
| Compatibility, accessibility and remaining tools | Incremental feature implementations | Expand beyond synthetic fixtures to a documented real-file corpus; test keyboard/focus behavior and cross-viewer output; turn remaining `gaps.md` entries into bounded slices. |
| Release readiness and final adversarial review | Applicable preceding slices | Verify packaged assets and installed behavior, resolve remaining license/source-availability items, review malformed inputs and failure paths, and publish only evidence-backed release notes. |

## Current assignments

On-page selection is accepted for the source build. The native and UI contract has combined-gate evidence for geometry, stale-response handling, caps and browser DOM selection; installed Windows clipboard behavior with real native IPC remains open. Split integration is next: native owns the reversible document operation and save-folder flow, UI owns the bounded dialog and result presentation, and management schedules the combined gate and review.

The signed 0.2.4 draft predates current source work. It must not be used as evidence that newly implemented tools shipped. No full Acrobat-parity or final-review completion claim is warranted while the remaining gaps are open.
