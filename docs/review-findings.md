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

Password tests also cover wrong/correct passwords, canceled or reused request tokens, rendering after unlock, delayed successful results after cancellation, cleared input, and simulated React effect replay. Native encryption fixtures currently cover RC4 V2; other encryption variants and Windows password-dialog interaction remain unverified.

Updater tests cover interrupted downloads, timeouts, retry, reset progress counters, busy-state restoration, blocked Close/Escape while installing, duplicate-click suppression, and resource cleanup. These use mocked updater calls and do not establish Windows installer recovery.

## Open risks and verification work

- File reads and PDFium's source copy have no total memory limit. Undo stores complete page plans without a history budget, and the worker queue is unbounded. Measure oversized files, long edit histories and rapid-scroll backlogs before setting resource limits. No out-of-memory crash was reproduced in this review.
- Matching page counts do not prove that two parsers repair a damaged page tree in the same order. More damaged-document fixtures are needed.
- Native parser crash isolation, fuzzing, a real-document compatibility corpus and sustained memory-pressure tests remain missing.
- Installed-app password/clipboard/restart behavior, the signed installer upgrade from 0.2.0, interrupted-install recovery and relaunch still require end-to-end verification.
- A complete dependency-license audit remains outstanding.

Combined automated gate: 43 frontend/release tests, 14 native tests, and the production frontend build passed. These checks exercise the current source; the 0.2.4 draft predates these changes.
