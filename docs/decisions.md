# Decisions

- The user's current-interface request supersedes the reference's classic-2020 menu/ribbon layout. Use a global toolbar, left All tools pane, floating quick actions, and right navigation. Original icons come from Lucide; app icon is original SVG. Adobe branding/assets are not included.
- Windows/Tauri 2, React 18, TypeScript, CSS Modules and native PDFium are the initial stack. No PDF bytes enter the frontend; only metadata and rendered PNG responses cross IPC. Mutations are deferred and will belong in Rust.
- No global state library or dialog framework is introduced for this small read-only milestone. Reassess when multiple tools and dialogs need shared state. This is a documented deviation from the reference's Zustand/Radix scaffold.
- PDFium is not inherently thread safe. One native worker owns all PDFium objects; multiple frontend requests do not call the engine concurrently. A process pool can be considered after measuring throughput and memory cost.
- Use the explicit Chromium 7881 PDFium binary release with pdfium-render 0.9.4. Preserve its license files with binaries. Dependency lockfiles pin the resolved dependency graph. Full license audit and distribution review remain pending.
- Rendering is viewport-limited; images are capped at 3000 px wide / 5000 px tall. Current zoom is 10–400%, not the reference's 1–6400%. High zoom needs a true tile renderer first.
- Synthetic fixtures are generated locally and contain no private customer data. They are not substitutes for a representative 200-document compatibility corpus.
- The source's office-export, certification, PDF/A, true-redaction, and performance claims remain requirements to investigate and prove, not capabilities provided automatically by the named libraries. LibreOffice's PDF import uses Draw and does not establish a reliable PDF-to-Word reconstruction pipeline.
- The built executable is an unsigned development build. MSI/NSIS installation, updates, virtual printer and Explorer integration are deferred.
