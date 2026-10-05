// Builds UwUMirror's downloads for the system it runs on: on Windows and
// macOS the app packed into the setup with Nyu in it, on Linux the app as
// .deb, .rpm and a portable folder.
//
//   pnpm build:setup                       for this machine
//   pnpm build:setup --target <triple>     another architecture (macOS:
//                                          universal-apple-darwin for both
//                                          Intel and Apple silicon, as the
//                                          release has it)
//
// Everything lands in target/installers/, under the names the release
// publishes — no version in them, so a link to the newest release's file
// stays the same forever:
//
//   Windows  UwUMirror-windows-x64-setup.exe           what people run
//            UwUMirror-windows-arm64-setup.exe         the same for ARM
//   macOS    UwUMirror-macos-universal.dmg             what people open
//   Linux    UwUMirror-linux-<arch>.deb                the app for apt/dpkg
//            UwUMirror-linux-<arch>.rpm                the app for dnf/zypper/rpm
//            UwUMirror-linux-<arch>-portable.tar.gz    unpack and run
//
// Nothing here signs anything, and nothing needs to: UwUMirror 0.1 has no
// updater, so there is no update signature, and there is no code-signing
// certificate either (macOS gets Apple's ad-hoc seal, see below). CI builds
// every one of these (.github/workflows/installers.yml) and a tag publishes
// them as a draft release.

import { execFileSync, execSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const run = (command, env = {}) =>
  execSync(command, { cwd: root, stdio: 'inherit', env: { ...process.env, ...env } });

const fail = (message) => {
  console.error(`\n✗ ${message}`);
  process.exit(1);
};

const targetIndex = process.argv.indexOf('--target');
const target = targetIndex > 0 ? process.argv[targetIndex + 1] : undefined;
const targetArg = target ? ` --target ${target}` : '';

const { version } = JSON.parse(
  readFileSync(join(root, 'apps/desktop/src-tauri/tauri.conf.json'), 'utf8'),
);
{
  const setup = JSON.parse(
    readFileSync(join(root, 'apps/setup/src-tauri/tauri.conf.json'), 'utf8'),
  );
  if (setup.version !== version) fail(`apps/setup says ${setup.version}, the app says ${version}.`);
}

// scrcpy's device server goes into every bundle, and tauri-build refuses to
// build the app while a resource it names is missing. The script checks the
// hash and does nothing when the right one is already there.
console.log('\n▸ Fetching scrcpy-server');
run('node scripts/fetch-scrcpy-server.mjs');
const scrcpyServer = join(root, 'apps/desktop/src-tauri/resources/scrcpy-server');

// AirPlay's sound needs FFmpeg; Windows and macOS carry their own, built by
// scripts/build-ffmpeg.sh (CI does that before this). Linux packages depend on
// the distribution's instead.
const ffmpeg = join(root, 'apps/desktop/src-tauri/resources/ffmpeg');
if (process.platform === 'win32' || process.platform === 'darwin') {
  const libraries = existsSync(ffmpeg)
    ? readdirSync(ffmpeg).filter((name) => /^(lib)?av(codec|util)[.-]\d+\.(dll|dylib)$/.test(name))
    : [];
  if (libraries.length !== 2) {
    fail(
      `Expected libavcodec and libavutil in ${ffmpeg}, found ${libraries.join(', ') || 'nothing'}. Run scripts/build-ffmpeg.sh first.`,
    );
  }
}

const release = join(root, 'target', ...(target ? [target] : []), 'release');
const bundles = join(release, 'bundle');
const out = join(root, 'target', 'installers');
mkdirSync(out, { recursive: true });

/** The architecture as release file names say it. */
function arch() {
  if (target === 'universal-apple-darwin') return 'universal';
  const name = target ?? `${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}-host`;
  return name.startsWith('aarch64') ? 'arm64' : 'x64';
}

/** A Tauri config override, as a file: quoting JSON on a command line is a trap on every shell. */
function configFile(name, config) {
  const path = join(mkdtempSync(join(tmpdir(), 'uwumirror-build-')), `${name}.json`);
  writeFileSync(path, JSON.stringify(config));
  return path;
}

function only(dir, test, what) {
  const found = existsSync(dir) ? readdirSync(dir).filter(test) : [];
  if (found.length !== 1) fail(`Expected one ${what} in ${dir}, found ${found.length}.`);
  return join(dir, found[0]);
}

const produced = [];

if (process.platform === 'win32') {
  console.log(`\n▸ Building UwUMirror ${version}`);
  run(`pnpm --filter @uwumirror/desktop tauri build --no-bundle${targetArg}`);
  const app = join(release, 'uwumirror-desktop.exe');
  if (!existsSync(app)) fail(`Missing ${app}`);

  // The program and, next to it, the server it pushes to Android phones and
  // its FFmpeg: a Windows program's resource folder is its own folder.
  console.log('\n▸ Packing it into the setup');
  run(`pnpm --filter @uwumirror/setup tauri build --no-bundle${targetArg}`, {
    UWUMIRROR_SETUP_PAYLOAD: app,
    UWUMIRROR_SETUP_SCRCPY_SERVER: scrcpyServer,
    UWUMIRROR_SETUP_FFMPEG: ffmpeg,
  });
  const setup = join(out, `UwUMirror-windows-${arch()}-setup.exe`);
  copyFileSync(join(release, 'uwumirror-setup.exe'), setup);
  produced.push(setup);
} else if (process.platform === 'darwin') {
  // Ad-hoc signed: no Apple developer ID, but a sealed bundle, which Apple
  // silicon insists on and which keeps the app intact through the setup.
  const macOS = { signingIdentity: '-', minimumSystemVersion: '11.0' };
  const apps = mkdtempSync(join(tmpdir(), 'uwumirror-apps-'));

  console.log(`\n▸ Building UwUMirror ${version}`);
  run(
    `pnpm --filter @uwumirror/desktop tauri build --bundles app${targetArg} --config "${configFile('app', { bundle: { macOS } })}"`,
  );
  execFileSync('ditto', [join(bundles, 'macos', 'UwUMirror.app'), join(apps, 'UwUMirror.app')]);

  console.log('\n▸ Packing it into the setup');
  const setupConfig = configFile('setup', {
    bundle: { active: true, targets: ['dmg'], macOS },
  });
  run(
    `pnpm --filter @uwumirror/setup tauri build --bundles dmg${targetArg} --config "${setupConfig}"`,
    {
      UWUMIRROR_SETUP_PAYLOAD: apps,
    },
  );
  const dmg = join(out, `UwUMirror-macos-${arch()}.dmg`);
  copyFileSync(
    only(join(bundles, 'dmg'), (name) => name.endsWith('.dmg'), 'disk image'),
    dmg,
  );
  produced.push(dmg);
  rmSync(apps, { recursive: true, force: true });
} else {
  // The AppImage, unpacked, is the portable folder: it runs from its AppDir,
  // brings its own WebKit and needs no FUSE.
  const apps = mkdtempSync(join(tmpdir(), 'uwumirror-apps-'));
  const unpackAppImage = (image, name) => {
    const work = mkdtempSync(join(tmpdir(), 'uwumirror-appimage-'));
    execFileSync('chmod', ['+x', image]);
    execFileSync(image, ['--appimage-extract'], { cwd: work, stdio: 'ignore' });
    renameSync(join(work, 'squashfs-root'), join(apps, name));
    rmSync(work, { recursive: true, force: true });
  };

  console.log(`\n▸ Building UwUMirror ${version}`);
  run(`pnpm --filter @uwumirror/desktop tauri build --bundles appimage${targetArg}`);
  unpackAppImage(
    only(join(bundles, 'appimage'), (name) => name.endsWith('.AppImage'), 'AppImage of UwUMirror'),
    'UwUMirror',
  );
  // The next build bundles into the same folders.
  rmSync(join(bundles, 'appimage'), { recursive: true, force: true });

  // The packages install system-wide, to /usr, as package `uwumirror`. Tauri
  // names the package after productName in kebab case, which would make
  // "UwUMirror" `uw-u-mirror`; the menu entry keeps saying UwUMirror through
  // the template in tauri.conf.json, and the AUR package repacks this .deb
  // under the same name.
  console.log(`\n▸ Packaging UwUMirror ${version} as .deb and .rpm`);
  run(
    `pnpm --filter @uwumirror/desktop tauri build --bundles deb,rpm${targetArg} --config "${configFile('packages', { productName: 'uwumirror' })}"`,
  );
  const deb = join(out, `UwUMirror-linux-${arch()}.deb`);
  copyFileSync(
    only(join(bundles, 'deb'), (name) => name.endsWith('.deb'), 'deb'),
    deb,
  );
  const rpm = join(out, `UwUMirror-linux-${arch()}.rpm`);
  copyFileSync(
    only(join(bundles, 'rpm'), (name) => name.endsWith('.rpm'), 'rpm'),
    rpm,
  );
  produced.push(deb, rpm);

  // One folder, UwUMirror/, that runs where it lands. tar, not zip: the
  // AppDir needs its modes and symlinks.
  console.log('\n▸ Packing the portable folder');
  const staging = mkdtempSync(join(tmpdir(), 'uwumirror-portable-'));
  const folder = join(staging, 'UwUMirror');
  execFileSync('cp', ['-a', join(apps, 'UwUMirror'), folder]);
  const launcher = join(folder, 'uwumirror');
  writeFileSync(
    launcher,
    '#!/bin/sh\n# Starts UwUMirror from this folder.\nhere=$(dirname "$(readlink -f "$0")")\nexec "$here/AppRun" "$@"\n',
  );
  chmodSync(launcher, 0o755);
  writeFileSync(
    join(folder, 'README.txt'),
    [
      `UwUMirror ${version}, portable`,
      '',
      'Start it with ./uwumirror (or ./AppRun) in this folder. Nothing is installed:',
      'the folder can live anywhere and be deleted when you are done.',
      '',
      'It brings its own WebKit. For sound it needs the system’s ALSA library',
      '(libasound2), for AirPlay sound FFmpeg’s libavcodec, for Android mirroring',
      'adb (package adb or android-tools), and for the picture WebKit’s GStreamer',
      'H.264 decoder (gst-libav, gst-plugins-good) — docs/install.md has the details.',
      '',
      'Phones find UwUMirror in your own network: allow it through the firewall',
      '(AirPlay listens on TCP 7000 and a few more ports, and mDNS on UDP 5353).',
      '',
      'To update, fetch the newest UwUMirror-linux-<arch>-portable.tar.gz from',
      'https://github.com/MinifyX/UwUMirror/releases',
      '',
    ].join('\n'),
  );
  const portable = join(out, `UwUMirror-linux-${arch()}-portable.tar.gz`);
  execFileSync('tar', [
    '--owner=0',
    '--group=0',
    '--numeric-owner',
    '-czf',
    portable,
    '-C',
    staging,
    'UwUMirror',
  ]);
  rmSync(staging, { recursive: true, force: true });
  produced.push(portable);
  rmSync(apps, { recursive: true, force: true });
}

for (const file of produced) console.log(`✧ ${file}`);
