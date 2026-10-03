import { createHash } from 'node:crypto';
import { readFile, writeFile, mkdir, copyFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function releaseManifest({ version, tag, repository, signature, filename, notes = '', pubDate = new Date().toISOString() }) {
  if (!/^\d+\.\d+\.\d+$/.test(version) || tag !== `v${version}`) throw new Error('A stable version and its matching v-prefixed tag are required.');
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) throw new Error('Repository must be owner/name.');
  if (filename !== `Vela_${version}_x64-setup.exe`) throw new Error('Unexpected Windows installer name.');
  const cleaned = signature.trim();
  if (!/^[A-Za-z0-9+/=]+$/.test(cleaned) || !Buffer.from(cleaned, 'base64').toString().startsWith('untrusted comment:')) throw new Error('A Tauri updater signature is required.');
  if (typeof notes !== 'string' || !Number.isFinite(Date.parse(pubDate))) throw new Error('Invalid release metadata.');
  return {
    version, notes, pub_date: new Date(pubDate).toISOString(),
    platforms: { 'windows-x86_64': { signature: cleaned, url: `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${filename}` } },
  };
}

export function releaseVersion(packageJson, tauriJson, cargoText, tag) {
  const section = cargoText.match(/^\[package\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  const cargoVersion = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const version = packageJson.version;
  if (version !== tauriJson.version || version !== cargoVersion || tag !== `v${version}` || !/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Package, desktop, Rust and tag versions must match.');
  return version;
}

export async function prepareRelease({ root = process.cwd(), bundleDir = 'src-tauri/target/release/bundle/nsis', outputDir = 'release', repository = 'diaomin66/Vela', tag, notesFile }) {
  const [packageText, tauriText, cargoText] = await Promise.all(['package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml'].map((file) => readFile(path.join(root, file), 'utf8')));
  const version = releaseVersion(JSON.parse(packageText), JSON.parse(tauriText), cargoText, tag);
  const filename = `Vela_${version}_x64-setup.exe`;
  const source = path.resolve(root, bundleDir);
  const destination = path.resolve(root, outputDir);
  const [installer, signature, notes] = await Promise.all([
    readFile(path.join(source, filename)), readFile(path.join(source, `${filename}.sig`), 'utf8'),
    notesFile ? readFile(path.resolve(root, notesFile), 'utf8') : Promise.resolve(`Vela ${version}`),
  ]);
  if (installer.length < 1024 || installer[0] !== 0x4d || installer[1] !== 0x5a) throw new Error('The release artifact is not a Windows executable.');
  const manifest = releaseManifest({ version, tag, repository, signature, filename, notes });
  const checksum = `${createHash('sha256').update(installer).digest('hex')}  ${filename}\n`;
  await mkdir(destination, { recursive: true });
  if (source !== destination) {
    await copyFile(path.join(source, filename), path.join(destination, filename));
    await copyFile(path.join(source, `${filename}.sig`), path.join(destination, `${filename}.sig`));
  }
  await writeFile(path.join(destination, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
  await writeFile(path.join(destination, `SHA256SUMS-${version}.txt`), checksum);
  return { version, filename, outputDir: destination };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const supported = new Set(['--tag', '--repository', '--bundle-dir', '--output-dir', '--notes-file']);
  const options = {};
  for (let i = 0; i < args.length; i += 2) {
    if (!supported.has(args[i]) || !args[i + 1] || args[i + 1].startsWith('--')) throw new Error('Use --tag vX.Y.Z [--repository owner/name] [--bundle-dir path] [--output-dir path] [--notes-file path].');
    options[args[i].slice(2).replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = args[i + 1];
  }
  console.log(JSON.stringify(await prepareRelease(options), null, 2));
}
