// The text of a GitHub release: what changed, in German and English, from
// release-notes/<version>.json, then which file is for which system.
//
//   node scripts/release-body.mjs <version> [--out <file>]
//
// CI's release job (.github/workflows/installers.yml) writes it for the draft
// release a tag creates. Without --out it prints it, to read before tagging.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const REPOSITORY = 'MinifyX/UwUMirror';

/** Every file a release carries, besides SHA256SUMS.txt. The release job checks for exactly these. */
export const RELEASE_FILES = [
  'UwUMirror-windows-x64-setup.exe',
  'UwUMirror-windows-arm64-setup.exe',
  'UwUMirror-macos-universal.dmg',
  'UwUMirror-linux-x64.deb',
  'UwUMirror-linux-x64.rpm',
  'UwUMirror-linux-x64-portable.tar.gz',
  'UwUMirror-linux-arm64.deb',
  'UwUMirror-linux-arm64.rpm',
  'UwUMirror-linux-arm64-portable.tar.gz',
];

/** The notes for a version, checked: both languages, neither empty. */
export function readNotes(version) {
  const file = join(root, 'release-notes', `${version}.json`);
  if (!existsSync(file)) throw new Error(`release-notes/${version}.json is missing.`);
  const notes = JSON.parse(readFileSync(file, 'utf8'));
  for (const language of ['de', 'en']) {
    if (typeof notes[language] !== 'string' || !notes[language].trim()) {
      throw new Error(`release-notes/${version}.json has no '${language}' text.`);
    }
  }
  return notes;
}

export function releaseBody(version, notes = readNotes(version)) {
  const guide = `https://github.com/${REPOSITORY}/blob/main/docs/install.md`;
  const code = (name) => `\`${name}\``;
  // [system (de), system (en), files]
  const rows = [
    ['Windows (x64)', 'Windows (x64)', code('UwUMirror-windows-x64-setup.exe')],
    ['Windows auf ARM', 'Windows on ARM', code('UwUMirror-windows-arm64-setup.exe')],
    [
      'macOS (Intel & Apple-Chip)',
      'macOS (Intel & Apple chip)',
      code('UwUMirror-macos-universal.dmg'),
    ],
    [
      'Ubuntu / Debian',
      'Ubuntu / Debian',
      `${code('UwUMirror-linux-x64.deb')} · ARM: ${code('UwUMirror-linux-arm64.deb')}`,
    ],
    [
      'Fedora / openSUSE',
      'Fedora / openSUSE',
      `${code('UwUMirror-linux-x64.rpm')} · ARM: ${code('UwUMirror-linux-arm64.rpm')}`,
    ],
    ['Arch Linux', 'Arch Linux', `AUR: ${code('uwumirror-bin')}`],
    [
      'Linux portabel',
      'Linux portable',
      `${code('UwUMirror-linux-x64-portable.tar.gz')} · ARM: ${code('…-arm64-portable.tar.gz')}`,
    ],
  ];
  const table = (column) =>
    ['| | |', '|---|---|', ...rows.map((row) => `| ${row[column]} | ${row[2]} |`)].join('\n');
  const de = [
    table(0),
    'Windows: warnt es („Der Computer wurde durch Windows geschützt“), **Weitere Informationen → Trotzdem ausführen**. Fragt beim ersten Start die Firewall, **private Netzwerke** erlauben.',
    'macOS: die `.dmg` öffnen und **UwUMirror Setup** starten. UwUMirror ist nicht bei Apple notarisiert (keine Developer ID): sagt macOS, es könne das Programm nicht prüfen, unter **Systemeinstellungen → Datenschutz & Sicherheit → Trotzdem öffnen** freigeben.',
    'Linux: `.deb` und `.rpm` installieren systemweit (für den Ton von AirPlay braucht es FFmpeg, für Android `adb`); die portable Version einfach entpacken und `./UwUMirror/uwumirror` starten.',
    'UwUMirror aktualisiert sich noch nicht selbst: Für eine neue Version einfach das neue Setup oder Paket über das alte installieren.',
  ];
  const en = [
    table(1),
    'Windows: if it warns that it "protected your PC", **More info → Run anyway**. If the firewall asks on the first start, allow **private networks**.',
    "macOS: open the `.dmg` and start **UwUMirror Setup**. UwUMirror isn't notarized by Apple (no developer ID): if macOS says it can't check the app, allow it under **System Settings → Privacy & Security → Open Anyway**.",
    'Linux: the `.deb` and `.rpm` install system-wide (AirPlay sound needs FFmpeg, Android needs `adb`); the portable one you just unpack and start with `./UwUMirror/uwumirror`.',
    "UwUMirror doesn't update itself yet: for a new version, install the new setup or package over the old one.",
  ];
  return [
    `## Deutsch\n\n${notes.de}\n`,
    `## English\n\n${notes.en}\n`,
    `## Herunterladen\n\n${de.join('\n\n')}\n\n[Anleitung](${guide})\n`,
    `## Downloads\n\n${en.join('\n\n')}\n\n[Install guide](${guide})\n`,
    'Prüfsummen · checksums: `SHA256SUMS.txt`\n',
  ].join('\n');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const args = process.argv.slice(2);
  const outAt = args.indexOf('--out');
  const out = outAt >= 0 ? args.splice(outAt, 2)[1] : undefined;
  const version = args[0]?.replace(/^v/, '');
  if (!version) {
    console.error('Usage: node scripts/release-body.mjs <version> [--out <file>]');
    process.exit(1);
  }
  let body;
  try {
    body = releaseBody(version);
  } catch (error) {
    console.error(`✗ ${error.message}`);
    process.exit(1);
  }
  if (out) writeFileSync(out, body);
  else process.stdout.write(body);
}
