/**
 * Small line icons for buttons, drawn on a 24 × 24 grid with round caps like
 * the title bar's gear. Inline SVG: the app loads nothing from outside.
 */

const PATHS = {
  home: 'M4 11.5 12 5l8 6.5 M6.5 9.5V19h11V9.5',
  phone: 'M7.5 3.5h9v17h-9z M11 17.5h2',
  tablet: 'M5 3.5h14v17H5z M11 17.5h2',
  laptop: 'M5 6h14v9H5z M3 18.5h18',
  android:
    'M7 10h10v8H7z M9 10a3 3 0 0 1 6 0 M9.5 7.5l-1-1.5 M14.5 7.5l1-1.5 M10 13h.01 M14 13h.01',
  cast: 'M3.5 9V5.5h17v13H14 M3.5 13a5.5 5.5 0 0 1 5.5 5.5 M3.5 16.5a2 2 0 0 1 2 2',
  qr: 'M4 4h6v6H4z M14 4h6v6h-6z M4 14h6v6H4z M14 14h2v2h-2z M18 18h2v2h-2z M14 18h2 M18 14h2',
  wifi: 'M4 10a11.5 11.5 0 0 1 16 0 M7 13.2a7.2 7.2 0 0 1 10 0 M10 16.4a2.9 2.9 0 0 1 4 0 M12 19.5h.01',
  usb: 'M12 3.5v15 M12 18.5a1.5 1.5 0 1 0 0 .1 M12 13l-4-2.5V8 M12 10.5l4-2.5V6 M7 8h2 M15 5h2v2h-2z',
  plus: 'M12 5v14 M5 12h14',
  refresh: 'M19.5 12a7.5 7.5 0 1 1-2.2-5.3 M19.5 4.5v4h-4',
  download: 'M12 4v11 M7 10.5l5 5 5-5 M5 19.5h14',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  close: 'M6 6l12 12 M18 6 6 18',
  stop: 'M7 7h10v10H7z',
  fullscreen: 'M4 9V4h5 M15 4h5v5 M20 15v5h-5 M9 20H4v-5',
  exitFullscreen: 'M9 4v5H4 M20 9h-5V4 M15 20v-5h5 M4 15h5v5',
  volume: 'M4.5 9.5h3l4.5-4v13l-4.5-4h-3z M15.5 9a4 4 0 0 1 0 6 M18 6.5a7.5 7.5 0 0 1 0 11',
  volumeOff: 'M4.5 9.5h3l4.5-4v13l-4.5-4h-3z M16 9.5l5 5 M21 9.5l-5 5',
  pause: 'M8.5 6v12 M15.5 6v12',
  unlink: 'M9.5 14.5l5-5 M8 11.5 6 13.5a3.5 3.5 0 0 0 5 5l2-2 M16 12.5l2-2a3.5 3.5 0 0 0-5-5l-2 2',
  sparkles:
    'M11 4.5c.6 3.4 2.1 4.9 5.5 5.5-3.4.6-4.9 2.1-5.5 5.5-.6-3.4-2.1-4.9-5.5-5.5 3.4-.6 4.9-2.1 5.5-5.5Z M18 14.5c.3 1.6 1 2.3 2.5 2.5-1.6.3-2.3 1-2.5 2.5-.3-1.5-1-2.2-2.5-2.5 1.5-.2 2.2-.9 2.5-2.5Z',
  info: 'M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17Z M12 11v5 M12 8h.01',
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({
  name,
  size = 16,
  title,
  className,
}: {
  name: IconName;
  size?: number;
  title?: string;
  className?: string;
}) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      className={className ? `icon ${className}` : 'icon'}
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
