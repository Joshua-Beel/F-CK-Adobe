# PDF Workstation

Windows desktop PDF application under development. The target is the current Acrobat workspace, following Joshua's explicit correction to the supplied classic-2020 reference. This is a viewer foundation, **not a complete clone or a finished Phase 0 acceptance**. Product branding remains undecided; the window uses a descriptive working label.

## Run

Use the desktop shortcut **PDF Workstation (development)**. The locally built executable is `src-tauri/target/debug/pdf-workstation.exe`. Keep its adjacent `resources` folder. It embeds the frontend and does not require Vite to run. Choose **Explore a sample PDF** or **Open a file**.

Development: install Node.js, Rust MSVC, Visual Studio C++ tools, and WebView2. Run `npm ci`, `npm run fixtures`, fetch PDFium using `scripts/setup-pdfium.ps1`, then `npm run tauri -- dev`. Open a new shell after installing Rust so Cargo is on PATH.

Build: `npm run tauri -- build --debug --no-bundle`.

Test: `npm test` and `cargo test --manifest-path src-tauri/Cargo.toml`.

## Implemented

- Tauri 2 / React 18 / TypeScript / CSS Modules desktop app with a native PDFium worker.
- Current-style global bar, document tabs, left All tools panel, floating quick tools, right navigation and zoom controls, Home, searchable tool catalog, light/dark themes.
- Open unencrypted PDFs through a native dialog; display native-rendered PNGs; continuous scrolling; hand pan; page jump; zoom 10–400%; fit width; close tabs; in-session file listing and stars.
- Virtualized page images with cleanup of blob URLs. Native LRU cache has a 512 MiB weighted budget including decoded-pixel estimates. PDFium calls run serially on a dedicated native thread.
- Ctrl+O, Ctrl+W, Ctrl+Tab, Ctrl+1, Ctrl+2, F4, Shift+F4, Home, End, Page Up, Page Down, H.

## Known gaps and next milestone

See [docs/gaps.md](docs/gaps.md). Editing, save, text selection, search, print, OCR, signatures, redaction, persistent recents, and thumbnail rendering are not implemented. Advanced tools are disabled. The page list uses page numbers, not thumbnail images. No source PDF is modified. No telemetry or document-upload service is included.

Next: complete viewer core (text selection/search, encrypted-file prompt, bookmarks/thumbnails, persisted preferences and recents), collect an authorized real-PDF corpus, benchmark the 98-page scan and 1,500-page abstract, and add packaged-app CI/E2E. Do not claim 60 fps or the source prompt's other performance targets from the synthetic tests.

## References

- User-supplied original prompt: [docs/reference-prompt.md](docs/reference-prompt.md), retained as reference content.
- Current UI baseline: https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/workspace.html
- PDFium threading constraints: https://docs.rs/pdfium-render/0.9.4/pdfium_render/
- Architecture decisions: [docs/decisions.md](docs/decisions.md).

## Recent changes

- Added the first Windows viewer foundation and current-style workspace, native worker/cache, synthetic fixtures, and viewport/rendering tests. Rust 1.98.1 was installed on the development machine. Detailed verification and remaining acceptance conditions are recorded in [docs/phase-0-status.md](docs/phase-0-status.md).
- Built the standalone development executable and added a desktop shortcut. Verified native sample rendering and next-page navigation; the file picker opens, but automated file selection remains unverified because of desktop automation limitations.
- Excluded Rust outputs and fixture files from Vite's watcher after Windows reported a locked compiled DLL. Repository commits use Joshua Beel's verified GitHub account with its GitHub no-reply address. No remote repository is configured yet.
