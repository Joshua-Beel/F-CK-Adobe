# Dependency license inventory

This inventory covers the current Windows x64 source tree and the locally extracted v0.2.4 draft installer. It identifies notice-packaging work; it does not mark the app's licensing review complete. Versions below come from the lockfiles and installed package manifests, not requested version ranges.

## What is packaged today

`src-tauri/tauri.conf.json` explicitly includes the PDFium DLL, its top-level `LICENSE`, all files under `resources/pdfium/licenses/`, and the welcome PDF. The extracted draft installer contains that PDFium license and all 15 upstream notice files. The bundle configuration does not include a general Rust/frontend third-party notice collection. JavaScript license comments exist in the current `dist` output, but that is not evidence that every full notice is included or available to users.

The PDFium distribution is Chromium build **151.0.7881.0**, pinned by `scripts/setup-pdfium.ps1`. Its local `args.gn` says Windows x64, standalone, V8 disabled, XFA disabled. Keep its complete upstream notice set rather than deriving a new list from the wrapper crate's license:

`abseil.txt`, `agg23.txt`, `fast_float.txt`, `freetype.txt`, `icu.txt`, `lcms.txt`, `libjpeg_turbo.ijg`, `libjpeg_turbo.md`, `libopenjpeg.txt`, `libpng.txt`, `libtiff.txt`, `llvm-libc.txt`, `pdfium.txt`, `simdutf.txt`, `zlib.txt`.

PDFium's version/build configuration files are present locally but are not in the bundle resource list. Record that version in the eventual notice index so the DLL and its notices remain traceable.

## Frontend production dependencies

These are the eight non-development entries in `package-lock.json`. A production bundler may remove unused code; this list is the conservative package inventory, not a claim that every package is present byte-for-byte in the final JavaScript.

| Package | Locked version | Declared license | Notice source in `node_modules` |
| --- | --- | --- | --- |
| `@tauri-apps/api` | 2.11.1 | Apache-2.0 OR MIT | `@tauri-apps/api/LICENSE_APACHE-2.0`, `LICENSE_MIT` |
| `@tauri-apps/plugin-updater` | 2.11.0 | MIT OR Apache-2.0 | npm package contains `LICENSE.spdx`; full matching-version texts exist in the cached Rust updater crate |
| `lucide-react` | 1.46.0 | ISC in package metadata | `lucide-react/LICENSE` also contains Feather-derived icon attribution and MIT text; retain the whole file |
| `react` | 18.3.1 | MIT | `react/LICENSE` |
| `react-dom` | 18.3.1 | MIT | `react-dom/LICENSE` |
| `scheduler` | 0.23.2 | MIT | `scheduler/LICENSE` |
| `loose-envify` | 1.4.0 | MIT | `loose-envify/LICENSE` |
| `js-tokens` | 4.0.0 | MIT | `js-tokens/LICENSE` |

The updater's `LICENSE.spdx` is metadata, not the full Apache/MIT text. Its generic `PackageName: tauri` should not be used as an exact inventory of that npm package's contents.

Frontend development tools are separately declared: Tauri CLI 2.11.4, TypeScript 5.8.3, Vite 6.4.3, Vitest 3.2.4, React Vite plugin 4.7.0, React test renderer 18.3.1, and React type packages. They are not application runtime dependencies just because they appear in the lockfile. Their installed development trees still need their own upstream notices if those trees or tools are redistributed.

## Direct Rust dependencies

The following versions and expressions were read through offline, locked Cargo metadata for `x86_64-pc-windows-msvc`. Preserve upstream expressions: `AND` and `OR` are not interchangeable.

| Runtime dependency | Locked version | Declared license |
| --- | --- | --- |
| `tauri` | 2.11.5 | Apache-2.0 OR MIT |
| `tauri-plugin-updater` | 2.11.0 | Apache-2.0 OR MIT |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 |
| `pdfium-render` | 0.9.4 | MIT OR Apache-2.0 |
| `image` | 0.25.10 | MIT OR Apache-2.0 |
| `rfd` | 0.15.4 | MIT |
| `tokio` | 1.53.1 | MIT |
| `lopdf` | 0.45.0 | MIT |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 |
| `windows` | 0.61.3 | MIT OR Apache-2.0 |

`tempfile` is used by the production atomic-save implementation, not only tests. `tauri-build` 2.6.3 is a build dependency (Apache-2.0 OR MIT). Procedural macros and build tools must be classified separately when generating the final inventory; simply copying every `Cargo.lock` package into a list of shipped libraries would overstate it.

## Rust transitive packages requiring more than a generic MIT notice

Walking normal dependency edges produced 340 third-party package identities. Excluding procedural-macro packages and their traversal produced 292. These are conservative metadata sets: feature unification and actual linker elimination still require release-build confirmation. Neither count is a verified count of linked libraries.

Examples that a notice generator must retain accurately:

| Package(s) | Declared expression / local evidence |
| --- | --- |
| `cssparser` 0.36.0, `dtoa-short` 0.3.5, `selectors` 0.36.1, `option-ext` 0.2.0 | MPL-2.0; still reachable after the procedural-macro exclusion. The first three occur through `dom_query`/CSS parsing; `option-ext` is a normal dependency of `dirs-sys`. Source-availability treatment remains an open release-review item. |
| `cssparser-macros` 0.6.1 | MPL-2.0; procedural-macro package, classify separately from shipped runtime code. |
| `ring` 0.17.14 | Apache-2.0 AND ISC. Its root `LICENSE` refers to `LICENSE-BoringSSL`, `LICENSE-other-bits`, and `src/polyfill/once_cell/LICENSE-APACHE` / `LICENSE-MIT`; copying the small root file alone loses the referenced texts. |
| `brotli` 8.0.4 | BSD-3-Clause AND MIT |
| `dpi` 0.1.2 | Apache-2.0 AND MIT |
| `encoding_rs` 0.8.41 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| `unicode-ident` 1.0.25 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| ICU4X, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `zerofrom`, `zerotrie`, `zerovec` | Unicode-3.0 across the resolved versions; preserve individual package identity and notice files. |
| `alloc-no-stdlib`, `alloc-stdlib`, `subtle` | BSD-3-Clause |
| `libloading`, `rustls-webpki`, `untrusted` | ISC |
| `foldhash`, `zlib-rs` | Zlib |

This table highlights exceptions; it is not the complete transitive package list. Cached crate sources are under `%USERPROFILE%/.cargo/registry/src/<registry>/<crate>-<version>/`. The manifests, complete license files, notice files, and any referenced subcomponent licenses there are the collection sources.

## Generated collection

`scripts/dependency-notices.mjs` collects the conservative resolved Windows Cargo graph and production npm lock entries into `src-tauri/resources/third-party-licenses/inventory.json` and `THIRD-PARTY-NOTICES.txt`. There are 367 package records, including build/auxiliary packages; this is not a count of libraries linked into the application. The collection includes Lucide's Feather attribution and nested `ring` notices.

Twelve missing local notices were resolved using published crate commits and official upstream license files. `scripts/notice-supplements/manifest.json` records exact commits, provenance URLs and SHA-256 hashes. Offline generation verifies those inputs. Explicitly running `scripts/fetch-notice-supplements.mjs --fetch` retrieves the recorded files and rejects unexpected hashes.

The generated files are included in Tauri resources and available through **Menu > Third-party notices** in the source build. The installer script checks freshness before building. Existing PDFium notices remain separately packaged.

To regenerate after a dependency change:

```powershell
cargo fetch --locked --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml
node scripts/dependency-notices.mjs
node scripts/dependency-notices.mjs --check
```

## Remaining release checks

1. Resolve the five recorded MPL source-availability review entries for the exact shipped versions. Record source archive locations/checksums and local modifications. Notice collection alone does not settle these requirements.
2. Extract a newly built installer and compare the packaged index and notice files against the generated manifest; check the installed app's notice entry. The old draft's PDFium contents do not verify this expanded collection.
3. Record the WebView2 bootstrapper/distribution version and its associated terms separately. The config downloads the bootstrapper; this inventory did not inspect that payload. The app's own redistribution license is also unresolved: the root has no `LICENSE` file and its Cargo package has no license field. Joshua owns that choice.

## Reproducing the metadata read

```text
cargo metadata --offline --locked --filter-platform x86_64-pc-windows-msvc --format-version 1 --manifest-path src-tauri/Cargo.toml
```

The initial inventory used local lockfiles, installed dependency notices and the extracted v0.2.4 draft's PDFium directory. The later supplement collection fetched the recorded upstream license files. Generated-notice checks establish reproducibility, not complete release-license review.
