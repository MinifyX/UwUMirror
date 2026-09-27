// Puts scrcpy's device server where the app bundle picks it up:
//
//   node scripts/fetch-scrcpy-server.mjs
//
// UwUMirror mirrors Android phones with the small server from scrcpy
// (https://github.com/Genymobile/scrcpy, Apache-2.0): it is pushed to the
// phone over adb and streams the screen back. Server and client must be the
// same version, so the version is pinned here and in
// crates/uwumirror-android/src/scrcpy.rs, and the download is checked against
// the hash from scrcpy's own SHA256SUMS.txt before anything is written.

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const SCRCPY_VERSION = '4.1';
const SHA256 = 'deacb991ed2509715160ffdc7907e47b4160eb30d1566217e9047fd5b8850cae';
const URL = `https://github.com/Genymobile/scrcpy/releases/download/v${SCRCPY_VERSION}/scrcpy-server-v${SCRCPY_VERSION}`;

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const target = join(root, 'apps/desktop/src-tauri/resources/scrcpy-server');

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

if (existsSync(target) && sha256(readFileSync(target)) === SHA256) {
  console.log(`scrcpy-server ${SCRCPY_VERSION} is already in place`);
  process.exit(0);
}

const response = await fetch(URL);
if (!response.ok) {
  console.error(`Could not download ${URL}: HTTP ${response.status}`);
  process.exit(1);
}
const bytes = Buffer.from(await response.arrayBuffer());
const actual = sha256(bytes);
if (actual !== SHA256) {
  console.error(
    `scrcpy-server ${SCRCPY_VERSION} has the wrong hash:\n  expected ${SHA256}\n  got      ${actual}`,
  );
  process.exit(1);
}
mkdirSync(dirname(target), { recursive: true });
writeFileSync(target, bytes);
console.log(`scrcpy-server ${SCRCPY_VERSION} → ${target}`);
