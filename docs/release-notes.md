## 0.2.6 (draft candidate)

- Adds on-page embedded-text selection and literal match highlights where supported geometry is available, plus Page text fallbacks for unsupported or capped pages.
- Adds fixed-size splitting, current-page crop, Combine Files, Insert Pages, and Replace Pages. Structural copy operations use current edits, preserve the stated source metadata, and create a new output without changing either source tab.
- Adds source-availability archives for five exact MPL-2.0 crates and current dependency notices to the bundled resources.

Native desktop verification of selection/clipboard behavior, split and structural-copy save dialogs, crop interaction, printing, installation, and upgrade/relaunch remains open.

## 0.2.5 (tagged; no draft artifacts)

- The release workflow stopped during npm tests because the clean runner had not fetched the locked Cargo registry required by the offline dependency-notice checks.
- No native test, signing, installer, asset upload, or GitHub draft-release step ran for this tag.

## 0.2.4

- Verifies the actual application extracted from the signed installer, rather than Tauri's restored unsigned build file.
- Includes embedded-text search, case matching, selectable excerpts, and links to matching pages.

## 0.2.3 (unreleased)

- Runs installer signature verification directly in PowerShell 7 on GitHub Actions.
- Includes embedded-text search and the Windows signing argument fix described below.

## 0.2.2 (unreleased)

- Find embedded PDF text with Ctrl+F, optional case matching, selectable excerpts, and links to matching pages.
- Stop a search while reading; results follow the current page order after edits.
- Fixed Windows signing command arguments containing spaces.

Search shows one excerpt per matching page, up to 500 pages. On-page selection, match highlighting, and OCR remain in development.

## 0.2.1 (unreleased)

- Windows publisher signing through Joshua Beel's Azure signing profile.
- Release builds verify timestamped signatures on the application and installer.
- Keeps the existing updater key so version 0.2.0 can accept this update.

## 0.2.0

- Windows installer with the PDF engine included.
- Menu > Check for updates downloads verified updates from GitHub releases.
- Update installation waits until every edited PDF has been saved or its edits discarded.
- PDF viewer and Organize Pages: rotate, move, delete, extract, undo/redo, and save a new copy.

Text editing, OCR, search, printing, and signatures remain in development.
