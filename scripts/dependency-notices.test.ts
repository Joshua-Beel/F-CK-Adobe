import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { expect, it } from 'vitest';
import { noticeFiles, runtimeCandidates, supplementalFiles } from './dependency-notices.mjs';

it('applies supplements only to exact versions and rejects changed source bytes', () => {
  const folder = mkdtempSync(join(tmpdir(), 'pdf-notice-supplement-'));
  const text = 'An exact upstream notice\n';
  writeFileSync(join(folder, 'LICENSE'), text, 'utf8');
  writeFileSync(join(folder, 'manifest.json'), JSON.stringify({ packages: { 'cargo:example@1.2.3': { crateCommit: 'a'.repeat(40), evidence: 'https://example.test/source', files: [{ file: 'LICENSE', url: 'https://example.test/license', sha256: createHash('sha256').update(text).digest('hex') }] } } }), 'utf8');
  expect(supplementalFiles(folder, 'cargo:example@1.2.4').files).toEqual([]);
  const result = supplementalFiles(folder, 'cargo:example@1.2.3');
  expect(result.files[0].text).toBe(text);
  expect(result.files[0].provenance.crateCommit).toBe('a'.repeat(40));
  writeFileSync(join(folder, 'LICENSE'), 'Changed', 'utf8');
  expect(() => supplementalFiles(folder, 'cargo:example@1.2.3')).toThrow('hash mismatch');
});

it('collects nested licenses and arbitrary license-directory names without copying ordinary source', () => {
  const folder = mkdtempSync(join(tmpdir(), 'pdf-notices-'));
  mkdirSync(join(folder, 'src', 'subcomponent'), { recursive: true });
  mkdirSync(join(folder, 'licenses'));
  writeFileSync(join(folder, 'LICENSE'), 'Root license\n', 'utf8');
  writeFileSync(join(folder, 'src', 'subcomponent', 'LICENSE-MIT'), 'Nested license\n', 'utf8');
  writeFileSync(join(folder, 'licenses', 'component.txt'), 'Other component\n', 'utf8');
  writeFileSync(join(folder, 'src', 'main.rs'), 'source excluded', 'utf8');
  const files = noticeFiles(folder);
  expect(files.map(file => file.path)).toEqual(['LICENSE', 'licenses/component.txt', 'src/subcomponent/LICENSE-MIT']);
  expect(files.every(file => /^[0-9a-f]{64}$/.test(file.sha256))).toBe(true);
  expect(files[2].text).toBe('Nested license\n');
});

it('keeps build edges and procedural macro descendants outside runtime candidates', () => {
  const pkg = (id: string, kind = 'lib') => ({ id, targets: [{ kind: [kind] }] });
  const dep = (pkg: string, kind: string | null = null) => ({ pkg, dep_kinds: [{ kind }] });
  const metadata = { packages: [pkg('root'), pkg('runtime'), pkg('build'), pkg('macro', 'proc-macro'), pkg('macro-helper')], resolve: { root: 'root', nodes: [
    { id: 'root', deps: [dep('runtime'), dep('build', 'build'), dep('macro')] },
    { id: 'runtime', deps: [] }, { id: 'build', deps: [] },
    { id: 'macro', deps: [dep('macro-helper')] }, { id: 'macro-helper', deps: [] },
  ] } };
  expect([...runtimeCandidates(metadata)]).toEqual(['runtime']);
});

it('bundles notices and exact MPL source archives without changing included license text', () => {
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  expect(config.bundle.resources).toContain('resources/third-party-licenses/inventory.json');
  expect(config.bundle.resources).toContain('resources/third-party-licenses/THIRD-PARTY-NOTICES.txt');
  expect(config.bundle.resources).toContain('resources/third-party-sources/*');
  const inventory = JSON.parse(readFileSync('src-tauri/resources/third-party-licenses/inventory.json', 'utf8'));
  const notices = readFileSync('src-tauri/resources/third-party-licenses/THIRD-PARTY-NOTICES.txt', 'utf8');
  const sourceManifest = JSON.parse(readFileSync('src-tauri/resources/third-party-sources/MANIFEST.json', 'utf8'));
  expect(inventory.packages.find(pkg => pkg.name === 'lucide-react').notices.length).toBeGreaterThan(0);
  expect(notices).toContain('Cole Bemis');
  expect(notices).toContain('src/polyfill/once_cell/LICENSE-APACHE');
  expect(sourceManifest.archives).toHaveLength(5);
  const cssparser = inventory.packages.find(pkg => pkg.ecosystem === 'cargo' && pkg.name === 'cssparser' && pkg.version === '0.36.0');
  expect(cssparser.sourceArchive.installedResourcePath).toBe('resources/third-party-sources/cssparser-0.36.0.crate');
  expect(notices).toContain("Packaged source archive: resources/third-party-sources/cssparser-0.36.0.crate (relative to the application's resource directory)");
});
