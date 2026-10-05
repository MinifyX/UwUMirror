import { useEffect, useState } from 'react';
import { onAirplayPairing, type AirplayPairing } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { showToast } from './Toasts';

/** How long a PIN is good for (`PIN_LIFETIME` in `airplay/src/pin.rs`). */
const PIN_LIFETIME_MS = 60_000;

type Shown = { pin: string; address: string };

function failureText(reason: Extract<AirplayPairing, { type: 'failed' }>['reason']): string {
  switch (reason) {
    case 'wrongPin':
      return t('Falsche PIN – das Gerät kann es gleich noch einmal versuchen.');
    case 'expired':
      return t('Die PIN ist abgelaufen. Bitte noch einmal verbinden.');
    case 'locked':
      return t('Zu viele falsche PINs. In einer Minute geht es wieder.');
    case 'broken':
      return t('Das Koppeln mit PIN ist abgebrochen.');
  }
}

/**
 * The PIN a device (a Mac, mostly) asks for before it mirrors, large enough
 * to read from across the room. A card at the top, not a dialog: something
 * else may be mirroring in front of it, and it must not take the keyboard.
 * It goes when the device has paired, failed, or a minute has passed.
 */
export function PinPrompt() {
  useLanguage();
  const [shown, setShown] = useState<Shown | null>(null);

  useEffect(() => {
    const off = onAirplayPairing((event) => {
      if (event.type === 'pinRequested') {
        setShown({ pin: event.pin, address: event.address });
        return;
      }
      setShown(null);
      if (event.type === 'paired') {
        showToast(t('Gekoppelt – {address} darf jetzt spiegeln.', { address: event.address }));
      } else {
        showToast(failureText(event.reason), 'error');
      }
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);

  useEffect(() => {
    if (!shown) return;
    const timer = window.setTimeout(() => setShown(null), PIN_LIFETIME_MS);
    return () => window.clearTimeout(timer);
  }, [shown]);

  if (!shown) return null;
  return (
    <div className="pin-prompt" role="status" aria-live="assertive">
      <div className="pin-prompt-text">
        <p className="pin-prompt-title">{t('Gib diese PIN auf dem Gerät ein:')}</p>
        <p className="pin-prompt-digits" aria-label={shown.pin.split('').join(' ')}>
          {shown.pin}
        </p>
        <p className="pin-prompt-meta">
          {t('{address} möchte über AirPlay spiegeln. Die PIN gilt eine Minute.', {
            address: shown.address,
          })}
        </p>
      </div>
      <button className="icon-button" onClick={() => setShown(null)} aria-label={t('Schließen')}>
        ×
      </button>
    </div>
  );
}
