/**
 * The English catalogue: German string → English string, one file per area of
 * the app. See `lib/i18n.ts`.
 */

import app from './app.json';
import home from './home.json';
import settings from './settings.json';
import stream from './stream.json';

export const EN: Readonly<Record<string, string>> = {
  ...app,
  ...home,
  ...settings,
  ...stream,
};
