import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const groups = [
  { packages: ['alloc-stdlib@0.2.4'], repo: 'dropbox/rust-alloc-no-stdlib', commit: 'ae42d22078b98549e987d2f03d12df7b984fde47', paths: ['LICENSE'] },
  { packages: ['defmt-parser@1.0.0'], repo: 'knurling-rs/defmt', commit: '4a8cdb44891ed57b8ff5a023b6bec7137c48708f', paths: ['LICENSE-MIT', 'LICENSE-APACHE'] },
  { packages: ['selectors@0.36.1'], repo: 'servo/stylo', commit: '635e1a19d02960588a00e189bd4bd5bdb150ec3d', paths: [], officialLicense: 'https://www.mozilla.org/media/MPL/2.0/index.txt', evidence: 'https://raw.githubusercontent.com/servo/stylo/635e1a19d02960588a00e189bd4bd5bdb150ec3d/selectors/lib.rs' },
  { packages: ['tauri-plugin@2.6.3'], repo: 'tauri-apps/tauri', commit: '6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a', paths: ['LICENSE_MIT', 'LICENSE_APACHE-2.0'] },
  { packages: ['unic-char-property@0.9.0', 'unic-char-range@0.9.0', 'unic-common@0.9.0', 'unic-ucd-version@0.9.0'], repo: 'open-i18n/rust-unic', commit: '5878605364af97a3358368a6eaef02104af2e016', paths: ['LICENSE-MIT', 'LICENSE-APACHE'] },
  { packages: ['unic-ucd-ident@0.9.0'], repo: 'open-i18n/rust-unic', commit: '8a6ce83063d90b91ae2ce59eddb803edd393fca9', paths: ['LICENSE-MIT', 'LICENSE-APACHE'] },
  { packages: ['webview2-com@0.38.2', 'webview2-com-sys@0.38.2'], repo: 'wravery/webview2-rs', commit: 'b74dc5e2b394044bea5191052868ce7a106c202c', paths: ['LICENSE'] },
  { packages: ['webview2-com-macros@0.8.1'], repo: 'wravery/webview2-rs', commit: 'dffa41a8a46d3f5565eefbff2de57d38d399f158', paths: ['LICENSE'] },
];

if (!process.argv.includes('--fetch')) throw new Error('Use --fetch explicitly to retrieve the pinned upstream notice supplements. Normal notice generation is offline.');
const destination = join(dirname(fileURLToPath(import.meta.url)), 'notice-supplements');
const recorded = JSON.parse(readFileSync(join(destination, 'manifest.json'), 'utf8'));
const expected = new Map(Object.values(recorded.packages).flatMap(entry => entry.files.map(file => [file.url, file.sha256])));
const fetched = await Promise.all(groups.map(async group => {
  const urls = group.officialLicense ? [group.officialLicense] : group.paths.map(path => `https://raw.githubusercontent.com/${group.repo}/${group.commit}/${path}`);
  const files = await Promise.all(urls.map(async (url, index) => {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`Cannot retrieve ${url}: HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    if (bytes.includes(0) || bytes.length < 500) throw new Error(`Unexpected license contents: ${url}`);
    const file = `${group.repo.split('/')[1]}-${group.commit.slice(0, 12)}-${group.paths[index] ?? 'MPL-2.0.txt'}`;
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    if (expected.get(url) !== sha256) throw new Error(`Upstream notice differs from the reviewed hash: ${url}`);
    return { file, url, sha256, bytes };
  }));
  return { ...group, files };
}));
mkdirSync(destination, { recursive: true });
const packages = {};
for (const group of fetched) {
  for (const file of group.files) writeFileSync(join(destination, file.file), file.bytes);
  for (const pkg of group.packages) packages[`cargo:${pkg}`] = {
    crateCommit: group.commit,
    evidence: group.evidence ?? `https://github.com/${group.repo}/tree/${group.commit}`,
    files: group.files.map(({ bytes, ...file }) => file),
  };
}
writeFileSync(join(destination, 'manifest.json'), JSON.stringify({ schema: 1, provenance: 'Published crate .cargo_vcs_info.json commits; official upstream license files. selectors references Mozilla MPL 2.0 in its exact-version source header.', packages }, null, 2) + '\n', 'utf8');
console.log(`Retrieved notice supplements for ${Object.keys(packages).length} exact package versions.`);
