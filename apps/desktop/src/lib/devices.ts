/**
 * How a stream is named and pictured in tabs and lists.
 */

import type { StreamState } from './api';
import { t } from './i18n';
import type { IconName } from '../components/Icon';

/** An icon for what is mirroring: iPhone, iPad, Mac, sound only, Android, a computer. */
export function streamIcon(stream: Pick<StreamState, 'kind' | 'model'>): IconName {
  if (stream.kind === 'android') return 'android';
  if (stream.kind === 'airplayaudio') return 'volume';
  if (stream.kind === 'cast') return 'monitor';
  const model = stream.model ?? '';
  if (model.startsWith('iPad')) return 'tablet';
  if (/^(Mac|iMac)/.test(model)) return 'laptop';
  return 'phone';
}

/** "AirPlay", "AirPlay-Ton", "Android", or what a sending computer runs on. */
export function streamSource(stream: Pick<StreamState, 'kind' | 'model'>): string {
  switch (stream.kind) {
    case 'airplay':
      return 'AirPlay';
    case 'airplayaudio':
      return t('AirPlay-Ton');
    case 'cast':
      return stream.model ?? t('Computer');
    default:
      return 'Android';
  }
}
