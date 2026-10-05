/**
 * Windows' firewall for this program, as a small store: Settings → Miracast
 * shows it and sets it up, and a Miracast stream whose picture doesn't come
 * points to it.
 *
 * Windows' own prompt on the first start opens private networks only, but
 * Miracast's Wi-Fi Direct link counts as public. Setting up asks once for an
 * administrator (UAC) and adds UwUMirror's two rules (see
 * `crates/uwumirror-firewall`). Elsewhere than on Windows `needed` is false.
 */

import { useSyncExternalStore } from 'react';
import { api, errorText, type FirewallStatus } from './api';
import { t } from './i18n';

type State = {
  status: FirewallStatus | null;
  /** The administrator prompt is open, or the rules are being written. */
  busy: boolean;
};

let state: State = { status: null, busy: false };
const listeners = new Set<() => void>();

function publish(next: Partial<State>) {
  state = { ...state, ...next };
  for (const listener of listeners) listener();
}

/** Reads the firewall again (no administrator needed). */
export async function refreshFirewall(): Promise<FirewallStatus | null> {
  try {
    const status = await api.firewallStatus();
    publish({ status });
    return status;
  } catch {
    return state.status;
  }
}

export type SetupOutcome = { ok: true } | { ok: false; declined: boolean; text: string };

/** Sets the firewall up: one UAC prompt. Then the store has the new state. */
export async function setUpFirewall(): Promise<SetupOutcome> {
  if (state.busy) return { ok: false, declined: false, text: '' };
  publish({ busy: true });
  try {
    const status = await api.firewallSetup();
    publish({ status, busy: false });
    return status.ready
      ? { ok: true }
      : {
          ok: false,
          declined: false,
          text: t('Eingerichtet, aber eine andere Regel blockiert UwUMirror noch.'),
        };
  } catch (error) {
    publish({ busy: false });
    void refreshFirewall();
    const text = errorText(error);
    return text === 'declined'
      ? {
          ok: false,
          declined: true,
          text: t('Abgebrochen. Ohne Administratorrechte bleibt die Firewall, wie sie ist.'),
        }
      : { ok: false, declined: false, text };
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The firewall's state; `status` is null until it was first read. */
export function useFirewall(): State {
  return useSyncExternalStore(subscribe, () => state);
}

/** Miracast's picture didn't come in time: Windows' player gives up with this code. */
export function looksLikeFirewall(reason: string | null): boolean {
  return !!reason && /0xc00d4278/i.test(reason);
}
