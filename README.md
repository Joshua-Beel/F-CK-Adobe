# PDF Workstation

A Windows PDF app being built as an alternative to Acrobat, with a familiar layout and tools that work locally. PDF Workstation is the working name for now.

You can already read PDFs, rearrange pages, and save your changes to a new file. There's still plenty to build before it can replace Acrobat for everyday work.

## Install

Download the Windows setup file from the [latest release](https://github.com/Joshua-Beel/F-CK-Adobe/releases/latest). The installer includes the PDF engine. Once installed, open a file or choose **Explore a sample PDF** to try it out.

For updates, use **Menu > Check for updates**. The app shows what's new and lets you choose when to install. Save any edited documents first; the app won't install an update while you have unsaved changes.

Version 0.2.0 has a signed update package, but no Windows publisher signature. The 0.2.4 draft installer and its packaged application both have verified, timestamped Joshua Beel publisher signatures. Installation and upgrade testing remain pending, so the draft has not been published. See [Releases and signing](docs/releases.md).

## What works

- **Reading:** open PDFs in tabs, scroll through pages, pan, jump to a page, zoom from 10% to 400%, or fit the page to the window width.
- **Search (source build):** Ctrl+F searches embedded PDF text, with optional case matching, selectable excerpts, and links to matching pages. Search follows page edits; scans need OCR first. Results show one excerpt per matching page, up to 500 pages.
- **Organizing pages:** select thumbnails or enter a page range, rotate pages, move a page earlier or later, delete pages, and extract a selection into a new PDF.
- **Saving:** undo and redo page edits, then use **Save a Copy**. Your original stays untouched, and the app asks before closing a document with unsaved edits.
- **Workspace:** light and dark themes and an All tools panel. The source build remembers theme, zoom, fit width, pan mode, and panel visibility across launches. Recent files and stars are still session-only.

Your PDFs stay on your computer. There's no document upload service or telemetry.

### Handy shortcuts

| Action | Shortcut |
| --- | --- |
| Open a PDF | Ctrl+O |
| Save a copy | Ctrl+S |
| Find text | Ctrl+F |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y |
| Close a tab / switch tabs | Ctrl+W / Ctrl+Tab |
| Actual size / fit width | Ctrl+1 / Ctrl+2 |
| Previous / next page | Page Up / Page Down |
| First / last page | Home / End |
| Toggle page list / tools panel | F4 / Shift+F4 |
| Toggle hand tool | H |

## What's still missing

Text and image editing, text selection directly on PDF pages, printing, OCR, signatures, and redaction aren't ready yet. Search excerpts can be selected and copied, but matches aren't highlighted on the page yet. Unavailable tools are disabled in the interface. Recent files and stars don't survive a restart yet, and password-protected PDFs aren't supported.

Saving currently means writing a new copy. You can't overwrite an existing file or save changes back to the original. Some page operations are also blocked on signed PDFs, forms, tagged documents, or files with bookmarks and annotations. The [Organize Pages guide](docs/tools/organize-pages.md) explains those limits.

Next up are the remaining reading basics: text selection on pages, search highlighting, bookmarks, better page navigation, and remembering your files and settings. There's also more testing to do with real documents. The generated 98-page scan and 1,500-page text file are useful test cases, but they don't tell us how every large PDF will behave. The full list is in [Known gaps](docs/gaps.md).

## Run from source

You'll need Windows, Node.js, Rust with the MSVC toolchain, Visual Studio C++ build tools, and WebView2. If you've just installed Rust, open a new terminal so `cargo` is available.

From the project folder, run these commands in PowerShell:

```powershell
npm ci
npm run fixtures
./scripts/setup-pdfium.ps1
npm run tauri -- dev
```

To build a standalone development executable:

```powershell
npm run tauri -- build --debug --no-bundle
```

You'll find it at `src-tauri/target/debug/pdf-workstation.exe`. Keep the `resources` folder beside it. This build runs without a development server. On Joshua's development machine, the **PDF Workstation (development)** shortcut opens it.

To run the tests:

```powershell
npm test
cargo test --manifest-path src-tauri/Cargo.toml
```

To build an installer, see [Releases and signing](docs/releases.md). That guide covers the local signing key, GitHub Actions secrets, and publishing updates.

## Under the hood

The app uses Tauri 2, React 18, TypeScript, and CSS Modules. Rust handles PDF work: PDFium renders pages on a dedicated thread, and lopdf writes edited copies. The viewer renders pages near the viewport and uses a native cache with a 512 MiB budget that accounts for both PNG data and estimated decoded pixels.

An open document keeps a snapshot of its original bytes. Before saving, the app checks that PDFium can open the output and that the page count is right, then writes a new file without replacing an existing one.

For more background, see the [architecture notes](docs/decisions.md), [PDFium documentation](https://docs.rs/pdfium-render/0.9.4/pdfium_render/), and [original project brief](docs/reference-prompt.md). The interface follows the [current Acrobat workspace](https://helpx.adobe.com/acrobat/desktop/get-started/learn-the-basics/workspace.html); the older layout in the brief is just a reference.

## Recent changes

- The source build now saves reading preferences locally: theme, zoom, fit mode, pan mode, and panel visibility. Invalid saved values fall back to defaults, and unavailable storage shows a session-only notice. All 19 frontend/release tests and the production build pass, including an app remount test and storage failure cases. These changes are newer than the 0.2.4 draft; native restart verification is still pending.
- The 0.2.4 release workflow passed all tests, signed the installer, verified its packaged application, and created a draft release. Downloaded assets were independently checked locally: both publisher signatures are valid and timestamped; installer SHA-256 is `cf97903897c10f0f02c31bc61f9ac9d929a9c5abdc116dc200cce01a4cd68c7a`, matching GitHub. Desktop automation still returns unreliable window captures, so installation/search UI checks and the full upgrade test remain unverified. The release stays a draft.
- The 0.2.3 signature check found Tauri's restored unsigned build file. CI now verifies the installer, extracts its packaged application using 7-Zip, and verifies that application's timestamped publisher signature. Prepared 0.2.4 for this check. Publishing and the upgrade test are still pending.
- Final completion requires a deep code review and adversarial tests: malformed PDFs, preservation of original files, undo/redo branches, stale search results, resource limits, crash behavior, and interrupted or invalid updates. Reproducible failures must be fixed with regression tests; unverified cases and remaining risks must be reported.
- Azure signing succeeded for 0.2.2, including the installer and updater signature, but CI stopped when the Windows PowerShell child process could not load its signature-verification module. Changed CI to call the build script directly in PowerShell 7 and prepared 0.2.3. Full signature verification, publishing, and the upgrade test remain pending.
- The 0.2.1 CI build passed tests and compilation but failed when invoking the signing tool. Fixed the signing configuration to pass the spaced application description as one argument, using Tauri's object notation. Prepared 0.2.2, including document search, for another signed build; installer and upgrade verification remain pending. The failed 0.2.1 tag is preserved.
- Added embedded-text search to the source build, with matching-page navigation, case matching, cancellation, selectable excerpts, and protection against stale results. All 15 frontend/release and 8 native tests pass; native extraction checks every page of all three valid fixtures. Production frontend and native debug builds pass. Desktop interaction verification is pending because window automation failed to acquire reliable input. This feature is not included in the already-tagged 0.2.1 signing build.
- Prepared 0.2.1 to exercise Azure publisher signing and the upgrade from 0.2.0. Release verification is in progress; this is not yet a verified published installer.
- Release signing uses protected repository Actions secrets and retains publisher and updater verification.
- Rewrote this README around installing and using the app, with clearer setup instructions and less development-log clutter.
- Connected future release builds to the existing Azure publisher profile. The build checks for valid, timestamped Joshua Beel signatures before preparing an update. The published 0.2.0 installer hasn't changed.
- Released the 0.2.0 installer and GitHub updater. Verified installation, PDF rendering, the live update check, and the downloaded installer's hash. Fixed update links to match GitHub's asset filenames. A full upgrade to a newer version still needs an end-to-end test. Details are in the [release guide](docs/releases.md).
- Added Organize Pages, undo/redo, Save a Copy, and prompts for unsaved edits. Tests compare rotated previews with reopened saved files and check that originals remain unchanged. The latest recorded checks passed all 11 frontend/release tests and 7 Rust tests; desktop rotation and copy export were also checked. See the [tool guide](docs/tools/organize-pages.md).
- Built the initial viewer, native PDF worker, page cache, and test fixtures. Added a standalone development build and shortcut, and fixed a Windows file-lock issue in the development watcher. Earlier checks are recorded in [Viewer foundation](docs/phase-0-status.md).
