# PDF Workstation

Windows desktop PDF application under development. The target is the current Acrobat workspace, following Joshua's explicit correction to the supplied classic-2020 reference. The viewer and initial Organize Pages tools are available; this is **not a complete clone or full phase acceptance**. Product branding remains undecided; the window uses a descriptive working label.

## Run

Windows installer: download the setup executable from [GitHub releases](https://github.com/Joshua-Beel/F-CK-Adobe/releases/latest). In the installed app, use **Menu > Check for updates** for signed updates. See [release instructions](docs/releases.md) for local builds, the signing-key backup, and the one-time GitHub Actions secret Joshua must configure.

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
- Organize Pages: real thumbnails loaded near the viewport, Ctrl/Shift multi-selection, page ranges, clockwise/counterclockwise rotation, confirmed page deletion, moving a selected page earlier/later, extraction, undo/redo, and Save a Copy. Ctrl+S saves a new copy; Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y undo/redo.
- All output writes go through Rust/lopdf. The original PDF is held as a native snapshot. New copies are checked with PDFium, flushed to a temporary file, then atomically published without overwriting existing files. Unsaved edits prompt before tab/window closure.

## Known gaps and next milestone

See [docs/gaps.md](docs/gaps.md). Text/image editing, search, print, OCR, signatures, redaction, and persistent recents remain unimplemented. Save a Copy and the initial page tools work; overwriting originals and incremental saves are not supported. The viewer's right-side page list still uses page numbers; the organizer has image thumbnails. Unsupported signed/encrypted or structurally complex PDFs return explicit errors for relevant edits. No source PDF is modified. No telemetry or document-upload service is included.

Next: complete viewer core (text selection/search, encrypted-file prompt, bookmarks/thumbnails, persisted preferences and recents), collect an authorized real-PDF corpus, benchmark the 98-page scan and 1,500-page abstract, and add packaged-app CI/E2E. Do not claim 60 fps or the source prompt's other performance targets from the synthetic tests.

## References

- User-supplied original prompt: [docs/reference-prompt.md](docs/reference-prompt.md), retained as reference content.
- Current UI baseline: https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/workspace.html
- PDFium threading constraints: https://docs.rs/pdfium-render/0.9.4/pdfium_render/
- Architecture decisions: [docs/decisions.md](docs/decisions.md).

## Recent changes

- Published and installed v0.2.0. Downloaded the public installer and verified its SHA-256 against the tested build; checked the live update endpoint from the installed app. Added the tag-triggered GitHub workflow for tested, signed draft releases. GitHub-hosted builds await Joshua's `TAURI_SIGNING_PRIVATE_KEY` Actions secret; local signed releases already work. A newer-version replacement/relaunch has not yet been exercised end to end.

- Matched update-manifest URLs to GitHub's uploaded asset names, which replace spaces with periods. Verified the uploaded installer digest matches the locally tested installer before publication.

- Added the 0.2.0 per-user Windows NSIS installer, bundled PDF engine, signed GitHub-release updater with release notes/progress and unsaved-document protection, and release-manifest validation. Signing keys remain outside Git. Windows Authenticode signing is not configured. Eleven frontend/release tests and seven native tests pass; the installer exits successfully and the installed app renders the bundled sample PDF.

- Added the first Windows viewer foundation and current-style workspace, native worker/cache, synthetic fixtures, and viewport/rendering tests. Rust 1.98.1 was installed on the development machine. Detailed verification and remaining acceptance conditions are recorded in [docs/phase-0-status.md](docs/phase-0-status.md).
- Built the standalone development executable and added a desktop shortcut. Verified native sample rendering and next-page navigation; the file picker opens, but automated file selection remains unverified because of desktop automation limitations.
- Excluded Rust outputs and fixture files from Vite's watcher after Windows reported a locked compiled DLL. Repository commits use Joshua Beel's verified GitHub account with its GitHub no-reply address. Remote: https://github.com/Joshua-Beel/F-CK-Adobe, branch `master`.
- Added Organize Pages and native safe-copy output with undo/redo and close guards. Seven Rust tests and five frontend tests pass, including pixel-identical previews/reopened rotated copies for every valid fixture and byte-for-byte preservation of originals. Attached Open/Save dialogs to the main window. Details: [docs/tools/organize-pages.md](docs/tools/organize-pages.md).
- Verified the standalone desktop organizer, rendered thumbnails, page rotation, native Save dialog, and successful copy export with the unsaved indicator cleared. The generated UI-test copy remains in ignored `artifacts/`.
