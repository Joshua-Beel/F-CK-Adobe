# Installers and GitHub updates

Download the Windows x64 setup executable from https://github.com/Joshua-Beel/F-CK-Adobe/releases/latest. Run it once; it installs for the current user and includes PDFium and its license notices. WebView2 is installed if needed, which requires internet access. The published 0.2.0 installer has no Windows publisher signature. Future release builds are configured for Joshua's existing Azure publisher profile, with repository credentials now saved. Tauri update signatures are separate and are enabled.

In the installed app, choose **Menu > Check for updates**. The dialog shows the installed version, newer release notes, and download progress. Installation requires a click and is disabled while any document has unsaved edits or an operation is running. The verified update starts the installer, closes the app, and relaunches it. Open documents are not automatically restored. Checks are manual; ordinary commits do not update installed apps.

The public update endpoint is `https://github.com/Joshua-Beel/F-CK-Adobe/releases/latest/download/latest.json`. It contains the version, notes, Windows x64 installer URL, and detached signature. Update downloads use HTTPS and the Tauri plugin verifies signatures with the public key embedded in the installed application. Failed checks or downloads show an error and allow retry. Downgrades are not offered.

## Signing key

Keep private signing material outside Git in secure storage, with a secure backup. The public updater verification key remains in `src-tauri/tauri.conf.json`.

The file's contents are saved as the repository Actions secret `TAURI_SIGNING_PRIVATE_KEY`, with Joshua's explicit approval. The initial key has no password; `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is unnecessary unless a password-protected key is used. Do not generate a replacement key for each release.

## Publish a new version

### Azure publisher signing setup

Configure Azure publisher signing with protected repository Actions secrets or local environment variables. Never commit signing configuration values.

Configure `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, and `TAURI_SIGNING_PRIVATE_KEY` in protected repository Actions secrets. Never paste credential values into tracked documentation.

The release workflow installs pinned `artifact-signing-cli` 0.11.0 and invokes `npm run installer -- -AzureSigning`. The Tauri signing hook applies Azure Authenticode signatures during bundling, before the final installer receives its Tauri updater signature. It then requires valid timestamped `Joshua Beel` signatures on both the app executable and installer before generating the manifest or uploading assets. Missing credentials or invalid signatures fail the release; the workflow never falls back to unsigned publishing. Local `npm run installer` remains a development build without Azure publisher signing. See [Tauri's Azure signing integration](https://v2.tauri.app/distribute/sign/windows/#azure-artifact-signing).

Publish publisher signing as a new version; do not replace the already published 0.2.0 installer, because its Tauri signature and manifest identify those exact bytes. Version 0.2.1 is prepared for the first Azure-enabled workflow run. Its signing and upgrade checks are pending.

### Release steps

1. Set the same new stable version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`; refresh both lockfiles. Update `docs/release-notes.md` and README recent changes.
2. Run `npm test`, `cargo test --manifest-path src-tauri/Cargo.toml`, and `npm run installer`. The installer command uses the local signing key or `TAURI_SIGNING_PRIVATE_KEY`; it writes the setup executable, `.sig`, and `latest.json` into `src-tauri/target/release/bundle/nsis/`.
3. Commit, push, and tag that exact commit `vMAJOR.MINOR.PATCH`. Push the tag. Once all signing secrets are configured, `.github/workflows/release.yml` tests, builds, signs, and creates a draft release. The workflow refuses tag/version mismatches and does not overwrite releases.
4. Install and exercise the draft installer. Publish the release when verified. Mark it as the latest stable release so installed apps see the new `latest.json`. Upload all three assets before publishing; never publish an incomplete manifest or replace an existing version's files.

Local fallback: use `gh release create` with the existing version tag, the three locally built assets, `--draft`, and `--notes-file docs/release-notes.md`. This needs no Actions secret. After testing, use `gh release edit vMAJOR.MINOR.PATCH --draft=false --latest`.

The first 0.2.0 installer establishes the updater. Earlier development executables have no updater and must install this release manually. Installation replaces app files only; source PDFs remain outside the installation folder.

## Verification

The 0.2.4 draft passed GitHub Actions run 35184652612. Its downloaded installer and extracted application both pass the local timestamped Joshua Beel signature check. Installer SHA-256: `cf97903897c10f0f02c31bc61f9ac9d929a9c5abdc116dc200cce01a4cd68c7a`, matching the GitHub asset digest. All three release assets are present. Installation, search UI behavior, and the end-to-end upgrade remain pending because desktop capture/input is unreliable; do not treat the draft as a verified published release.

v0.2.0 was installed successfully, and its bundled sample rendered in the installed application. The live GitHub manifest was fetched and its installer downloaded; SHA-256 matched the locally tested build (`17788cb82bb42deac35ea197a385a9bafc8cd331422446a2b834e115a90c4d2e`). The installed updater checked the public endpoint and correctly reported no newer version. Eleven frontend/release tests and seven native tests pass, including unsaved-edit blocking and mocked network/signature failures. Those tests also pass after the 0.2.1 version change. A full newer-version replacement/relaunch and the GitHub-hosted signing workflow still need verification.
