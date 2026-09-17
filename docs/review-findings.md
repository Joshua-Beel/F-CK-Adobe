# Development review

This review covers the current frontend, native worker, page editor, IPC, update dialog and release scripts. It is an incremental review of the source build, not completion of the requested final review or certification of the published installer.

## Fixed with regression coverage

| Finding | Change and evidence |
| --- | --- |
| A malformed middle page ended PDFium iteration silently, exposing only the preceding pages. | Opening now loads every declared page by index and rejects any failure. A fixture with page 2 replaced by a null object is rejected through both opening paths. |
| Different source page counts from PDFium and lopdf could turn Save a Copy into unintended page removal. | Edit sessions retain the original page count. Editing and export reject parser count disagreements; deliberate deletion remains supported. |
| Empty opening passwords let lopdf decrypt automatically, bypassing the old encrypted-file guard. | The editor checks both current encryption and retained decryption state. Actual encrypted fixtures with empty and nonempty passwords reject editing and export and retain identical source bytes. |
| Confirmation-dialog keystrokes could trigger background open/save/undo. | Global shortcuts ignore dialog targets. All three regression tests failed before the guard and pass afterward, while workspace shortcuts still work. |
| The viewer could accept page counts beyond the IPC's 16-bit page index. | Opening rejects documents over 65,536 pages. The limit is a compatibility guard; a huge-document performance claim has not been established. |
| Repeated page-object references could rotate multiple saved pages when only one preview page was edited. | Editing and export reject duplicate leaf references. Regression checks unchanged source, plan, saved baseline, revision and history, plus ordinary-file success. |
| A delayed tab-close response could clear a different active tab or leave an active ID with no document. | Closing holds a synchronous guard and busy state until completion; regression tests cover rapid actions, failures, retry and discard confirmation. |
| PDFium signature-count errors could appear as 65,535 signatures through the wrapper's unsigned cast. | Properties return unknown for the sentinel and never claim signature validity. |
| Direct text selection could drift from cropped or rotated rendered pages, accept stale geometry, or expose a partial text layer. | Native returns all-or-nothing normalized glyph geometry for supported cardinal orientations, carries document/page/revision identity, restores temporary page rotation, and caps source text at 20,000 characters. The UI waits for an image, rejects mismatched or malformed replies, measures each browser text range into its native bound, disables selection in pan mode, and routes unsupported/truncated pages to Page text. Native coverage checks crop offsets, all four rotations, edits, stale/closed/out-of-range requests, whitespace and Unicode; focused UI tests cover stale replies, plain text, dimensions, hand mode and fallback. The combined gate passed 66 frontend/release tests, 38 native tests, and the production build. A mocked-IPC Vite harness verified browser selection `A B\né` and matching ranges at 0° and 90°; it is not Windows clipboard or real-native-IPC evidence. |

Password tests also cover wrong/correct passwords, canceled or reused request tokens, rendering after unlock, delayed successful results after cancellation, cleared input, and simulated React effect replay. Native encryption fixtures cover RC4-128 V2, AES-128 V4 and AES-256 V5 with protected and empty passwords, rendering every fixture page. Other encryption variants and Windows password-dialog interaction remain unverified.

Updater tests cover interrupted downloads, timeouts, retry, reset progress counters, busy-state restoration, blocked Close/Escape while installing, duplicate-click suppression, and resource cleanup. These use mocked updater calls and do not establish Windows installer recovery.

## Open risks and verification work

- File reads and PDFium's source copy have no total memory limit, and the worker queue is unbounded. Undo/redo snapshot allocations now share a 32 MiB per-document budget with oldest-history eviction; tests cover transfers, branching, oversized snapshots and preservation of current/source/saved states. Measure oversized files and rapid-scroll backlogs next. No out-of-memory crash was reproduced in this review.
- Matching page counts do not prove that two parsers repair a damaged page tree in the same order. More damaged-document fixtures are needed.
- Native parser crash isolation, fuzzing, a real-document compatibility corpus and sustained memory-pressure tests remain missing.
- Installed-app password/clipboard/restart behavior, including on-page selection with real native geometry, native print output and driver failures, the signed installer upgrade from 0.2.0, interrupted-install recovery and relaunch still require end-to-end verification.
- The [dependency inventory](license-inventory.md) records current license evidence. General dependency notices are generated and included in the bundle configuration; new installer extraction, MPL source availability and the project's own license remain unresolved.

Current combined automated gate: 66 frontend/release tests, 38 native tests, the production frontend build, and the standalone Windows debug build passed. These checks exercise the current source; the 0.2.4 draft predates these changes.
