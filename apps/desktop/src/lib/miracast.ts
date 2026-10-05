/**
 * The Miracast receiver's state, as a small store: the start page shows it,
 * and a stream that waits for its first picture shows the PIN a sender asks
 * for.
 *
 * The receiver runs in Rust (Windows only) and follows the settings; it
 * reports its changes as `miracast` events.
 */

import { useSyncExternalStore } from 'react';
import { api, errorText, onMiracastStatus, type MiracastStatus } from './api';
import { getSettings } from './settings';

let status: MiracastStatus | null = null;
const listeners = new Set<() => void>();

function publish(next: MiracastStatus) {
  status = next;
  for (const listener of listeners) listener();
}

let listening = false;
/** Counts the applies, so that an older one's late answer is ignored. */
let applied = 0;

/** Starts or stops the receiver to match the settings, and follows it. */
export async function applyMiracast() {
  if (!listening) {
    listening = true;
    await onMiracastStatus(publish);
  }
  const mine = ++applied;
  const { miracastEnabled, miracastAudio } = getSettings();
  try {
    const next = await api.miracastApply({ enabled: miracastEnabled, audio: miracastAudio });
    if (mine === applied) publish(next);
  } catch (error) {
    if (mine === applied)
      publish({ state: 'failed', name: '', pin: null, error: errorText(error) });
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The receiver's state; null until it is first known. */
export function useMiracast(): MiracastStatus | null {
  return useSyncExternalStore(subscribe, () => status);
}
