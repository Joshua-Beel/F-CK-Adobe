import { describe, expect, it } from 'vitest';
// @ts-expect-error Node release tooling is intentionally plain JavaScript.
import { createManifest } from './release-manifest.mjs';
describe('release manifest', () => {
  it('points the Windows updater at the exact versioned installer asset', () => {
    const value = createManifest('0.2.0', 'PDF Workstation_0.2.0_x64-setup.exe', ' signed\n', 'Release notes', '2026-09-17T00:00:00Z');
    expect(value.platforms['windows-x86_64']).toEqual({ signature: 'signed', url: 'https://github.com/Joshua-Beel/F-CK-Adobe/releases/download/v0.2.0/PDF%20Workstation_0.2.0_x64-setup.exe' });
    expect(value.version).toBe('0.2.0');
  });
  it('refuses an unsigned package, wrong architecture, or prerelease', () => {
    expect(() => createManifest('0.2.0', 'app_x64-setup.exe', '', '')).toThrow();
    expect(() => createManifest('0.2.0', 'app_arm64-setup.exe', 'sig', '')).toThrow();
    expect(() => createManifest('0.2.0-beta', 'app_x64-setup.exe', 'sig', '')).toThrow();
  });
});
