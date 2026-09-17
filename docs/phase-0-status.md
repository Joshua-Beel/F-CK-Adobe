# Phase 0 status

State: viewer foundation implemented; full reference acceptance remains incomplete.

Verified so far:
- `npm run build`: TypeScript check and production frontend build pass.
- `npm test`: three tests pass (navigation bounds, mixed page geometry, viewport raster bounds for 1,500 pages).
- `cargo test`: two tests pass. They cover native cache eviction/closure, rendering all 98 synthetic scan pages, opening the 6-page sample and 1,500-page text fixture, rendering their last pages, rejecting invalid page indexes, rejecting invalid PDF input, and rejecting rendering after closure.
- Browser inspection: Home, complete disabled tool catalog, OCR tool filtering, light/dark theme switch, no recorded browser errors.

- `tauri build --debug --no-bundle`: standalone Windows executable built with embedded frontend and adjacent PDFium resources.
- Built-app visual check: bundled six-page PDF opens and renders through native IPC; next-page control changes to page 2. Ctrl+O opens the Windows file picker. Automated file selection could not be completed because the desktop automation provider returned inconsistent dialog focus/geometry; a file-picker end-to-end pass is not claimed. The test process was stopped after verification to clear the dialog.
- Source encoding scan: no NUL bytes, replacement characters, or checked mojibake patterns in 32 source/configuration/documentation files.

Pending: complete native file-picker interaction and zoom/pan UI checks, real 98-page scan and 1,500-page abstract benchmarks, 60 fps proof, 200-file corpus, CI/E2E, full dependency-license audit and signed installer. Synthetic PDF results do not establish those conditions.
