import { t } from './i18n';

/**
 * Which system the app runs on, for the few words and keys that differ.
 *
 * The webview's user agent says it plainly on all three: WebView2 on Windows,
 * WKWebView on macOS, WebKitGTK on Linux.
 */

export type Platform = 'windows' | 'macos' | 'linux';

export function platform(): Platform {
  const agent = `${navigator.userAgent} ${navigator.platform ?? ''}`.toLowerCase();
  if (agent.includes('win')) return 'windows';
  if (agent.includes('mac')) return 'macos';
  return 'linux';
}

/** "Windows", "macOS" or "Linux", as the system calls itself. */
export function systemName(): string {
  switch (platform()) {
    case 'windows':
      return 'Windows';
    case 'macos':
      return 'macOS';
    default:
      return 'Linux';
  }
}

/**
 * A shortcut as the system writes it: "⌘," on a Mac, "Ctrl+," elsewhere
 * (Strg in German). `key` is what follows the modifier.
 */
export function shortcut(key: string): string {
  return platform() === 'macos' ? `⌘${key}` : `${t('Strg')}+${key}`;
}

/** The full-screen shortcut: F11 on Windows and Linux, ⌃⌘F on a Mac. */
export function fullscreenShortcut(): string {
  return platform() === 'macos' ? '⌃⌘F' : 'F11';
}
