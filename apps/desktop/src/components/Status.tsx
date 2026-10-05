/**
 * The receivers' states in words, shared by the start page (a pill at a
 * glance) and the settings (the full sentence).
 */

import type { AirplayStatus, CastStatus, MiracastStatus } from '../lib/api';
import { t } from '../lib/i18n';

/** Mint when devices can come, pink while starting, grey when off, amber on trouble. */
export type Tone = 'online' | 'starting' | 'off' | 'error';

export type ReceiverState = { tone: Tone; text: string };

export function airplayState(
  enabled: boolean,
  status: AirplayStatus | null,
  name: string,
): ReceiverState {
  if (!enabled) return { tone: 'off', text: t('Aus – iPhones sehen UwUMirror gerade nicht.') };
  if (status?.error)
    return {
      tone: 'error',
      text: t('AirPlay konnte nicht starten: {error}', { error: status.error }),
    };
  if (status?.running) return { tone: 'online', text: t('Empfangsbereit als „{name}“', { name }) };
  return { tone: 'starting', text: t('Startet…') };
}

export function castState(
  enabled: boolean,
  status: CastStatus | null,
  name: string,
): ReceiverState {
  if (!enabled)
    return { tone: 'off', text: t('Aus – andere Computer sehen UwUMirror gerade nicht.') };
  if (status?.error)
    return {
      tone: 'error',
      text: t('Der Empfang konnte nicht starten: {error}', { error: status.error }),
    };
  if (status?.running) return { tone: 'online', text: t('Empfangsbereit als „{name}“', { name }) };
  return { tone: 'starting', text: t('Startet…') };
}

export function miracastState(enabled: boolean, status: MiracastStatus | null): ReceiverState {
  if (!enabled)
    return { tone: 'off', text: t('Aus – Handys und PCs sehen UwUMirror gerade nicht.') };
  const name = status?.name || t('dieser Computer');
  switch (status?.state) {
    case 'listening':
      return { tone: 'online', text: t('Empfangsbereit als „{name}“', { name }) };
    case 'connected':
      return { tone: 'online', text: t('Verbunden – ein Gerät spiegelt über Miracast.') };
    case 'off':
      return { tone: 'off', text: t('Aus – Handys und PCs sehen UwUMirror gerade nicht.') };
    case 'noWifiDirect':
      return {
        tone: 'error',
        text: t(
          'Dieser Computer kann kein Miracast: Sein WLAN-Adapter (oder dessen Treiber) kann kein Wi-Fi Direct.',
        ),
      };
    case 'wifiOff':
      return {
        tone: 'error',
        text: t(
          'WLAN ist aus. Miracast braucht WLAN an diesem Computer – mit einem Netz verbunden sein muss er nicht.',
        ),
      };
    case 'disabledByPolicy':
      return { tone: 'error', text: t('Eine Richtlinie verbietet das Projizieren auf diesen PC.') };
    case 'busy':
      return {
        tone: 'error',
        text: t(
          'Windows gibt den Empfang gerade nicht her – projiziert dieser PC selbst auf einen anderen Bildschirm?',
        ),
      };
    case 'failed':
      return {
        tone: 'error',
        text: t('Miracast konnte nicht starten: {error}', { error: status.error ?? '?' }),
      };
    default:
      return { tone: 'starting', text: t('Startet…') };
  }
}

/** A state as a sentence with a dot, for the settings. */
export function StatusLine({ state }: { state: ReceiverState }) {
  return (
    <p className="status-line" data-state={state.tone}>
      <i className="dot" aria-hidden />
      {state.text}
    </p>
  );
}

/** A state in one word, for the start page; the full sentence is its tooltip. */
export function StatusPill({ state }: { state: ReceiverState }) {
  const word = {
    online: t('Bereit'),
    starting: t('Startet…'),
    off: t('Aus'),
    error: t('Problem'),
  }[state.tone];
  return (
    <span className="pill" data-state={state.tone} title={state.text}>
      <i className="dot" aria-hidden />
      {word}
    </span>
  );
}
