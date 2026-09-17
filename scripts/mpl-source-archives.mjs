import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { homedir } from 'node:os';
import { gunzipSync } from 'node:zlib';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const slash = value => value.replaceAll('\\', '/');
const normalizeLines = value => value.replaceAll('\r\n', '\n');
const archiveFolder = 'src-tauri/resources/third-party-sources';
const registrySource = 'registry+https://github.com/rust-lang/crates.io-index';

export function offlineCargoMetadata(root) {
  const cargo = process.env.CARGO ?? join(homedir(), '.cargo', 'bin', process.platform === 'win32' ? 'cargo.exe' : 'cargo');
  const result = spawnSync(cargo, ['metadata', '--offline', '--locked', '--filter-platform', 'x86_64-pc-windows-msvc', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml'], { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, windowsHide: true });
  if (result.error || result.status !== 0) throw new Error(`Offline Cargo metadata failed. First populate the cache with cargo fetch --locked --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml. ${result.error?.message ?? result.stderr}`);
  return JSON.parse(result.stdout);
}

function readManifest(root) {
  const manifest = JSON.parse(readFileSync(join(root, 'scripts/mpl-source-archives.manifest.json'), 'utf8'));
  if (manifest.schema !== 1 || !Array.isArray(manifest.archives) || manifest.archives.length !== 5) throw new Error('MPL source archive manifest must contain exactly five entries.');
  const names = new Set();
  for (const archive of manifest.archives) {
    if (!/^[a-z0-9][a-z0-9-]*$/.test(archive.name) || !/^\d+\.\d+\.\d+$/.test(archive.version) || !/^[0-9a-f]{64}$/.test(archive.sha256) || archive.url !== `https://static.crates.io/crates/${archive.name}/${archive.name}-${archive.version}.crate`) throw new Error(`Invalid MPL source archive manifest entry: ${archive.name ?? 'unknown'}.`);
    const key = `${archive.name}@${archive.version}`;
    if (names.has(key)) throw new Error(`Duplicate MPL source archive manifest entry: ${key}.`);
    names.add(key);
  }
  return manifest.archives.sort((left, right) => left.name.localeCompare(right.name, 'en'));
}

function lockPackages(root) {
  const lock = readFileSync(join(root, 'src-tauri/Cargo.lock'), 'utf8');
  const blocks = (`\n${lock}`).split(/\r?\n\[\[package\]\]\r?\n/).slice(1);
  const packages = new Map();
  for (const block of blocks) {
    const get = field => block.match(new RegExp(`^${field} = "([^"]+)"$`, 'm'))?.[1];
    const name = get('name'), version = get('version');
    if (name && version) packages.set(`${name}@${version}`, { source: get('source'), checksum: get('checksum') });
  }
  return packages;
}

function pathInArchive(name, version) { return `${name}-${version}.crate`; }

function tarText(bytes, offset, length) {
  const terminator = bytes.indexOf(0, offset);
  return bytes.subarray(offset, terminator === -1 || terminator > offset + length ? offset + length : terminator).toString('utf8');
}

function tarNumber(bytes, offset, length) {
  const text = tarText(bytes, offset, length).trim();
  if (!/^[0-7]*$/.test(text)) throw new Error('Invalid tar entry size.');
  return text ? Number.parseInt(text, 8) : 0;
}

export function archiveFileHashes(bytes) {
  const tar = gunzipSync(bytes);
  const files = new Map();
  for (let offset = 0; offset < tar.length;) {
    const header = tar.subarray(offset, offset + 512);
    if (header.length !== 512) throw new Error('Truncated tar header.');
    if (header.every(byte => byte === 0)) break;
    const size = tarNumber(header, 124, 12);
    const paddedSize = Math.ceil(size / 512) * 512;
    if (offset + 512 + paddedSize > tar.length) throw new Error('Truncated tar entry.');
    const name = `${tarText(header, 345, 155)}${tarText(header, 345, 155) ? '/' : ''}${tarText(header, 0, 100)}`;
    const type = header[156] || 48;
    if (type === 48) {
      if (!name || name.startsWith('/') || name.split('/').some(part => part === '..') || files.has(name)) throw new Error(`Unsafe or duplicate tar path: ${name}.`);
      files.set(name, sha256(tar.subarray(offset + 512, offset + 512 + size)));
    } else if (![53, 120, 103].includes(type)) {
      throw new Error(`Unsupported tar entry type for ${name || 'unnamed entry'}.`);
    }
    offset += 512 + paddedSize;
  }
  return files;
}

function sourceFileHashes(folder) {
  const files = new Map();
  function visit(current) {
    for (const entry of readdirSync(current, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name, 'en'))) {
      const path = join(current, entry.name);
      const itemPath = slash(relative(folder, path));
      if (entry.isSymbolicLink()) throw new Error(`Unexpected symbolic link in cached crate: ${itemPath}.`);
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile() && itemPath !== '.cargo-ok') files.set(itemPath, sha256(readFileSync(path)));
    }
  }
  visit(folder);
  return files;
}

function compareSourceTree(archive, archiveBytes, sourceDir) {
  const prefix = `${archive.name}-${archive.version}/`;
  const archiveFiles = archiveFileHashes(archiveBytes);
  const expected = new Map();
  for (const [path, value] of archiveFiles) {
    if (!path.startsWith(prefix)) throw new Error(`Archive contains a file outside ${prefix}: ${path}.`);
    expected.set(path.slice(prefix.length), value);
  }
  const actual = sourceFileHashes(sourceDir);
  for (const [path, value] of expected) {
    if (actual.get(path) !== value) throw new Error(`Cached crate source differs from archive: ${archive.name}@${archive.version} (${path}).`);
  }
  for (const path of actual.keys()) if (!expected.has(path)) throw new Error(`Cached crate source has an unexpected file: ${archive.name}@${archive.version} (${path}).`);
}

function sourceDirectoryFor(pkg) {
  const sourceDir = dirname(pkg.manifest_path);
  const registry = basename(dirname(sourceDir));
  const registryRoot = dirname(dirname(dirname(sourceDir)));
  return { sourceDir, cacheDir: join(registryRoot, 'cache', registry) };
}

function sourceEntries(root, packages, metadata) {
  const lock = lockPackages(root);
  const packageByKey = new Map(metadata.packages.map(pkg => [`${pkg.name}@${pkg.version}`, pkg]));
  return packages.map(archive => {
    const key = `${archive.name}@${archive.version}`;
    const lockEntry = lock.get(key);
    if (!lockEntry || lockEntry.source !== registrySource || lockEntry.checksum !== archive.sha256) throw new Error(`Cargo.lock does not match the MPL source archive manifest: ${key}.`);
    const pkg = packageByKey.get(key);
    if (!pkg || pkg.source !== registrySource) throw new Error(`Offline Cargo metadata does not contain the locked registry crate: ${key}.`);
    const { sourceDir, cacheDir } = sourceDirectoryFor(pkg);
    const cachePath = join(cacheDir, pathInArchive(archive.name, archive.version));
    if (!existsSync(cachePath)) throw new Error(`Cached source archive is missing: ${cachePath}. Run cargo fetch --locked before packaging.`);
    const bytes = readFileSync(cachePath);
    if (sha256(bytes) !== archive.sha256) throw new Error(`Cached source archive hash does not match Cargo.lock: ${key}.`);
    compareSourceTree(archive, bytes, sourceDir);
    const filename = pathInArchive(archive.name, archive.version);
    return { ...archive, cachePath, repositoryPath: `${archiveFolder}/${filename}`, installedResourcePath: `resources/third-party-sources/${filename}` };
  });
}

function generatedManifest(entries) {
  return JSON.stringify({
    schema: 1,
    coverage: 'Exact crates.io source archives for the five MPL-2.0 entries recorded in the dependency inventory. These files do not determine the application license or complete release licensing review.',
    archives: entries.map(({ cachePath, ...entry }) => entry),
  }, null, 2) + '\n';
}

function requireExactOutput(folder, entries, manifest) {
  if (!existsSync(folder)) throw new Error(`Packaged MPL source archives are missing: ${folder}.`);
  const expected = new Set([...entries.map(entry => pathInArchive(entry.name, entry.version)), 'MANIFEST.json']);
  for (const entry of readdirSync(folder, { withFileTypes: true })) {
    if (!entry.isFile() || !expected.has(entry.name)) throw new Error(`Unexpected or stale packaged MPL source archive resource: ${entry.name}.`);
  }
  for (const entry of entries) {
    const path = join(folder, pathInArchive(entry.name, entry.version));
    if (!existsSync(path)) throw new Error(`Packaged MPL source archive is missing: ${entry.name}@${entry.version}.`);
    const bytes = readFileSync(path);
    if (sha256(bytes) !== entry.sha256) throw new Error(`Packaged MPL source archive hash mismatch: ${entry.name}@${entry.version}.`);
  }
  const manifestPath = join(folder, 'MANIFEST.json');
  if (!existsSync(manifestPath) || normalizeLines(readFileSync(manifestPath, 'utf8')) !== normalizeLines(manifest)) throw new Error('Packaged MPL source archive manifest is missing or stale.');
}

export function prepareMplSourceArchives(root, metadata, check = false, options = {}) {
  const entries = sourceEntries(root, options.archives ?? readManifest(root), metadata);
  const folder = options.folder ?? join(root, archiveFolder);
  const manifest = generatedManifest(entries);
  if (check) requireExactOutput(folder, entries, manifest);
  else {
    mkdirSync(folder, { recursive: true });
    for (const entry of entries) copyFileSync(entry.cachePath, join(folder, pathInArchive(entry.name, entry.version)));
    writeFileSync(join(folder, 'MANIFEST.json'), manifest, 'utf8');
  }
  return entries.map(({ cachePath, ...entry }) => entry);
}

export function checkedSourceArchiveEntries(root, metadata) {
  return prepareMplSourceArchives(root, metadata, true);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const entries = prepareMplSourceArchives(root, offlineCargoMetadata(root), process.argv.includes('--check'));
  console.log(`${process.argv.includes('--check') ? 'Verified' : 'Packaged'} ${entries.length} exact MPL source archives.`);
}
