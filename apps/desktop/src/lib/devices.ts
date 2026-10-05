/**
 * How a stream is named and pictured in tabs and lists.
 */

import type { StreamState } from './api';
import { t } from './i18n';
import type { IconName } from '../components/Icon';

/** An icon for what is mirroring: iPhone, iPad, Mac, sound only, Android. */
export function streamIcon(stream: Pick<StreamState, 'kind' | 'model'>): IconName {
  if (stream.kind === 'android') return 'android';
  if (stream.kind === 'airplayaudio') return 'volume';
  // A phone or a PC: Miracast doesn't say which.
  if (stream.kind === 'miracast') return 'cast';
  const model = stream.model ?? '';
  if (model.startsWith('iPad')) return 'tablet';
  if (/^(Mac|iMac)/.test(model)) return 'laptop';
  return 'phone';
}

/** "AirPlay", "AirPlay-Ton", "Miracast" or "Android". */
export function streamSource(stream: Pick<StreamState, 'kind'>): string {
  switch (stream.kind) {
    case 'airplay':
      return 'AirPlay';
    case 'airplayaudio':
      return t('AirPlay-Ton');
    case 'miracast':
      return 'Miracast';
    default:
      return 'Android';
  }
}
