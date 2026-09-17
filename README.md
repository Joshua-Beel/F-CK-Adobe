# PDF Workstation

A Windows PDF app being built as an alternative to Acrobat, with a familiar layout and tools that work locally. PDF Workstation is the working name for now.

You can already read PDFs, rearrange pages, and save your changes to a new file. There's still plenty to build before it can replace Acrobat for everyday work.

## Install

Download the Windows setup file from the [latest release](https://github.com/Joshua-Beel/F-CK-Adobe/releases/latest). The installer includes the PDF engine. Once installed, open a file or choose **Explore a sample PDF** to try it out.

For updates, use **Menu > Check for updates**. The app shows what's new and lets you choose when to install. Save any edited documents first; the app won't install an update while you have unsaved changes.

Version 0.2.0 has a signed update package, but no Windows publisher signature. The 0.2.4 draft installer and its packaged application both have verified, timestamped Joshua Beel publisher signatures. Installation and upgrade testing remain pending, so the draft has not been published. See [Releases and signing](docs/releases.md).

## What works

- **Reading:** open PDFs in tabs, scroll through pages, pan, jump to a page, zoom from 10% to 400%, or fit the page to the window width. The source build restores each open tab's last page when switching documents; this position lasts for the session.
- **Passwords (source build):** enter a PDF's opening password, retry it, or cancel. Passwords are not saved. Encrypted PDFs remain read-only, including files with an empty opening password.
- **Document properties (source build):** Ctrl+D shows embedded description metadata, original file size, current page sizes, reported permissions and signature count. Unknown values stay unknown; signatures are not validated.
- **Printing (source build, awaiting native verification):** Ctrl+P opens a Windows printer dialog for all pages, the current page, or ranges. The app sends page images with current edits, fitted to the printable area. Encrypted files are blocked. Physical output and driver cancellation still need testing.
- **Page text (source build):** open the current page's embedded text, select any portion or all of it, and copy with Ctrl+C. Reading order depends on the PDF; scanned images still need OCR.
- **On-page text selection (source build):** choose the pointer tool to select supported embedded text directly over rendered pages. The native layer maps cropped, cardinally rotated glyph bounds to the displayed image after it loads; pan mode disables selection. Pages with unsupported, malformed, over-cap, or no positioned geometry show an explicit route to Page text instead. The viewer still virtualizes off-screen pages. Windows desktop clipboard behavior remains unverified.
- **Bookmarks (source build):** browse up to 1,000 embedded bookmarks with nested indentation and jump to supported internal page destinations. Unsupported actions stay disabled; destination zoom/position and bookmark editing are not implemented.
- **Search (source build):** Ctrl+F searches embedded PDF text, with optional case matching, selectable excerpts, next/previous matching-page navigation, and highlights on visible pages with supported text geometry. Search follows page edits; scans need OCR first. Results show one excerpt per matching page, up to 500 pages.
- **Organizing pages:** select thumbnails or enter a page range, rotate pages, move a page earlier or later, delete pages, and extract a selection into a new PDF. The source build also adds direct Move to page, Split, and Crop: fixed-size files from the current edited page order go into a new Windows-selected folder, and one selected page can be cropped with point insets. Cropping hides content; it does not redact it.
- **Saving:** undo and redo page edits, then use **Save a Copy**. Your original stays untouched, and the app asks before closing a document with unsaved edits. The source build limits undo/redo snapshots to 32 MiB per document, dropping the oldest history when needed.
- **Workspace:** light and dark themes and an All tools panel. The source build remembers theme, zoom, fit width, pan mode, panel visibility, and up to 50 recent file locations with stars across launches. Clear file history removes this list without deleting PDFs. Unsaved edits are not restored after restart.

Your PDFs stay on your computer. There's no document upload service or telemetry.

### Handy shortcuts

| Action | Shortcut |
| --- | --- |
| Open a PDF | Ctrl+O |
| Save a copy | Ctrl+S |
| Find text | Ctrl+F |
| Print (source build) | Ctrl+P |
| Document properties (source build) | Ctrl+D |
| Undo / redo | Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y |
| Close a tab / switch tabs | Ctrl+W / Ctrl+Tab |
| Actual size / fit width | Ctrl+1 / Ctrl+2 |
| Previous / next page | Page Up / Page Down |
| First / last page | Home / End |
| Toggle page list / tools panel | F4 / Shift+F4 |
| Toggle hand tool | H |

## What's still missing

Text and image editing, OCR, signatures, and redaction aren't ready yet. On-page selection and search highlights are limited to supported embedded text with cardinal glyph orientation; skewed, mirrored, malformed, capped, or no-positioned-geometry pages route to Page text or Find results. Scanned pages normally have no embedded text, which Page text reports explicitly. Native printing still needs end-to-end output verification. Unavailable tools are disabled in the interface.

Saving currently means writing a new copy. You can't overwrite an existing file or save changes back to the original. Some page operations are also blocked on signed PDFs, forms, tagged documents, or files with bookmarks and annotations. The [Organize Pages guide](docs/tools/organize-pages.md) explains those limits.

Next up is combining two open PDFs into a new copy, plus native verification of printing, search geometry, clipboard behavior, split folder selection, and crop results. There's also more testing to do with real documents. The generated 98-page scan and 1,500-page text file are useful test cases, but they don't tell us how every large PDF will behave. The full list is in [Known gaps](docs/gaps.md) and [development roadmap](docs/development-roadmap.md).

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

- Organize Pages now starts from the page you are viewing and can crop one selected page with point insets and a resulting-size preview. Cropping hides content rather than redacting it; invalid or oversized insets, stale documents, failures, and duplicate submissions leave the session intact. The source gate passed 87 frontend/release tests, 52 native tests, the production frontend build, and the standalone Windows debug build; a mocked-IPC browser harness checked the crop preview and normalized request, while native tests checked rendered-pixel and source preservation. See the [Organize Pages guide](docs/tools/organize-pages.md) and [review findings](docs/review-findings.md).
- Search now highlights literal matches directly on visible pages with supported glyph geometry while preserving Select copy behavior; Pan keeps highlights but disables selection. Query changes, stopped work, document tabs, and revisions clear or constrain overlay state, and a matching page whose geometry cannot locate text says so instead of drawing a guessed box. The final source gate passed 80 frontend/release tests and the production build; a mocked-IPC browser harness verified 0°/90° boxes and selected `A B\né`, not Windows clipboard or real native IPC. See [review findings](docs/review-findings.md) and the [development roadmap](docs/development-roadmap.md).
- Fixed Page text and Find so cropped or rotated pages retain visible headings and exclude text outside the crop. The regression failed before the fix and passed across all 16 original/edited rotation combinations afterward; all 45 native tests pass.
- Added fixed-size PDF splitting in Organize Pages. It writes the current edited page order to a new Windows-selected folder only after every output validates; source bytes, unsaved state, and undo/redo history remain unchanged. The combined source gate passed 72 frontend/release tests, 44 native tests, the production frontend build, and the standalone Windows debug build; native folder-dialog interaction remains unverified. See the [Organize Pages guide](docs/tools/organize-pages.md) and [review findings](docs/review-findings.md) for output, preservation, and size limits.
- Added on-page text selection for supported embedded text: Select enables it, Pan disables it, and unsupported, capped, malformed, or no-positioned-geometry pages offer Page text. The combined gate passed 66 frontend/release tests, 38 native tests, the production frontend build, and the standalone Windows debug build; a mocked-IPC browser harness verified aligned ranges and selected `A B\né`, but not the Windows clipboard or real native IPC. The feature is not included in published 0.2.0 or the 0.2.4 draft; see [review findings](docs/review-findings.md) and the [development roadmap](docs/development-roadmap.md).
- Added Document properties, including metadata, current page dimensions and reported permissions. Unknown signature counts stay unknown instead of displaying a library error as 65,535 signatures. Fixed a close-tab race that could leave a blank workspace, and blocked edits/export on malformed PDFs that reference the same page object twice. Added a reproducible 367-package dependency-notice collection, an app menu entry and an installer freshness check. All 59 frontend/release tests, 32 native tests, the production build and the standalone Windows debug build pass. Both copied notice resources match their source SHA-256 hashes. MPL source availability, the project's own license and new installer verification remain open; see the [inventory](docs/license-inventory.md).
- Added native Windows printing in the source build: printer/range selection, bounded page-image rendering from a stable edited-document snapshot, cancellation and cleanup on failures. Printing never clears unsaved edits and rejects encrypted files. Review fixed early/late cancellation races and a page-limit mismatch. Undo/redo snapshots now have a combined 32 MiB budget per document. All 49 frontend/release tests, 27 native tests and the production build pass. The [dependency inventory](docs/license-inventory.md) records license evidence and remaining packaging work. Native printer output, driver behavior and installer upgrade remain unverified; see [printing notes](docs/printing-plan.md). These changes are newer than the 0.2.4 draft.
- Tab switching now restores each document's last viewed page and clamps it after page edits shorten the document. Expanded native password tests to six RC4-128, AES-128 and AES-256 fixtures with protected and empty passwords, checking every rendered page, retries, cancellation, edit/export rejection and original-byte preservation. All 45 frontend/release tests, 14 native tests and the production build pass. Native interaction remains unverified. Printing remains disabled; [implementation notes](docs/printing-plan.md) describe the native path needed before enabling it.
- Parallel password and review work added opening-password prompts with retry/cancel, transient passwords, and cleanup after canceled opens. Encrypted files remain read-only, including empty-password files that lopdf automatically decrypts. Fixed silently truncated malformed PDFs, blocked edits/export when PDF engines disagree on the source page count, limited opening to the IPC-supported 65,536 pages, and disabled background shortcuts inside confirmation dialogs. Added interrupted-update/retry tests. All 43 frontend/release tests, 14 native tests and the production build pass. Native password interaction and the installer upgrade remain unverified. See [review findings](docs/review-findings.md) for remaining risks; these source changes are newer than the 0.2.4 draft.
- Recent files and stars now persist locally, capped at 50 entries. Clicking a saved entry reopens it or activates its existing tab; missing files show an error. Clear file history removes saved locations without deleting PDFs. Tests cover invalid storage, duplicate entries, app remounts, reopening and missing files. All 33 frontend/release tests, 11 native tests and the production build pass. Native restart verification remains pending; this feature is newer than the 0.2.4 draft.
- Added next/previous matching-page controls and highlighted literal matches in search excerpts. Navigation wraps through the available results and resets when the query or case option changes. New tests cover navigation, delayed results after Stop, and the 500-page result limit; all 29 frontend/release tests and the production build pass. Highlighting on rendered pages and native desktop verification remain pending. This change is newer than the 0.2.4 draft.
- Added Move to page in Organize Pages: choose one page, enter its final position, and move it using the existing undoable operation. Invalid positions are rejected and failed moves preserve selection. Fixed Shift+click selecting nonexistent pages after deletion. All 26 frontend/release tests and the production build pass; native desktop interaction with the new controls remains unverified. These changes are newer than the 0.2.4 draft.
- Fixed an export overflow found with adversarial PDF rotation values. The regression reproduced a debug-build panic; export now normalizes the original rotation before adding the requested turn. Extreme positive and negative values round-trip correctly, and all 11 native tests pass across the existing fixtures. This source fix is newer than the 0.2.4 draft; the full review and installed-app checks remain open.
- Added a page-text dialog for selecting embedded text and copying with Ctrl+C. It distinguishes empty pages from extraction errors and displays PDF text without interpreting it as HTML. All 23 frontend/release tests and the production build pass. Native clipboard interaction is still unverified; this feature is newer than the 0.2.4 draft and does not provide selection directly over rendered pages.
- Adversarial bookmark tests exposed repeated PDFium initialization and interference between parallel native workers: tests found inconsistent bookmark counts, preview mismatches, and startup failures. All service handles now share one process-wide worker, serializing initialization and PDF operations. Added cases for cyclic outlines, 1,005 bookmarks, empty outlines, closed documents, and internal GoTo actions; bookmark results remain capped at 1,000. This is an incremental hardening pass, not completion of the requested full review.
- Added a native bookmark pane with internal page navigation, nested entries, an explicit 1,000-entry cap, and disabled unsupported destinations. All 21 frontend/release tests, 9 native tests, and the production build pass, including nested bookmark extraction, external-action rejection, stale requests, and UI navigation. Native desktop interaction remains unverified. This change is newer than the 0.2.4 draft.
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
