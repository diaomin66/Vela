import { describe, expect, it } from 'vitest';
// @ts-expect-error Release tooling is intentionally a standalone Node ESM script.
import { releaseManifest, releaseVersion } from '../scripts/release-manifest.mjs';

const fixture = { productName: 'Vela', version: '0.4.0', tag: 'v0.4.0', repository: 'diaomin66/Vela', filename: 'Vela_0.4.0_x64-setup.exe', signature: Buffer.from('untrusted comment: fixture\nfixture-only').toString('base64'), pubDate: '2026-10-03T00:00:00Z', notes: '更新说明' };
describe('signed release metadata', () => {
  it('keeps the existing update feed while binding the renamed installer', () => {
    const release = { ...fixture, productName: 'AhaX', version: '0.7.0', tag: 'v0.7.0', filename: 'AhaX_0.7.0_x64-setup.exe' };
    expect(releaseManifest(release).platforms['windows-x86_64'].url).toBe('https://github.com/diaomin66/Vela/releases/download/v0.7.0/AhaX_0.7.0_x64-setup.exe');
    expect(() => releaseManifest({ ...release, filename: 'Vela_0.7.0_x64-setup.exe' })).toThrow();
  });
  it('pins downloads to the exact release and Windows target', () => {
    const result = releaseManifest(fixture);
    expect(result.version).toBe('0.4.0');
    expect(result.platforms['windows-x86_64'].url).toBe('https://github.com/diaomin66/Vela/releases/download/v0.4.0/Vela_0.4.0_x64-setup.exe');
    expect(result.platforms['windows-x86_64'].signature).toBe(fixture.signature);
    expect(result.notes).toBe('更新说明');
  });
  it.each([
    { tag: 'v0.3.0' }, { version: '0.4.0-beta' }, { repository: 'owner/name/extra' },
    { filename: '../untrusted.exe' }, { signature: '' }, { signature: 'not-a-signature' }, { pubDate: 'invalid' },
  ])('rejects mismatched or incomplete release metadata %j', (changed) => {
    expect(() => releaseManifest({ ...fixture, ...changed })).toThrow();
  });
  it('requires desktop, Rust, package and tag versions to agree', () => {
    const cargo = '[package]\nname = "vela"\nversion = "0.4.0"\n\n[dependencies]\nfoo = "1"\n';
    expect(releaseVersion({ version: '0.4.0' }, { version: '0.4.0' }, cargo, 'v0.4.0')).toBe('0.4.0');
    expect(() => releaseVersion({ version: '0.4.0' }, { version: '0.3.0' }, cargo, 'v0.4.0')).toThrow();
    expect(() => releaseVersion({ version: '0.4.0' }, { version: '0.4.0' }, cargo.replace('0.4.0', '0.3.0'), 'v0.4.0')).toThrow();
  });
});
