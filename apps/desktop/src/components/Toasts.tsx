import { useSyncExternalStore } from 'react';
import { t, useLanguage } from '../lib/i18n';

/** A button in a toast; clicking it also closes the toast. */
export type ToastAction = { label: string; run: () => void };

export type Toast = { id: number; text: string; tone: 'info' | 'error'; action?: ToastAction };

let toasts: Toast[] = [];
let next = 1;
const listeners = new Set<() => void>();

function publish(list: Toast[]) {
  toasts = list;
  for (const listener of listeners) listener();
}

export function dismissToast(id: number) {
  publish(toasts.filter((toast) => toast.id !== id));
}

/** A short note in the corner; it goes away by itself after a few seconds. */
export function showToast(text: string, tone: Toast['tone'] = 'info', action?: ToastAction) {
  const id = next++;
  publish([...toasts.slice(-3), { id, text, tone, action }]);
  // One with a button stays a little longer, to be clicked.
  window.setTimeout(() => dismissToast(id), action ? 15000 : tone === 'error' ? 9000 : 5000);
}

export function Toasts() {
  useLanguage();
  const list = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => toasts,
  );
  return (
    <div className="toasts" role="status" aria-live="polite">
      {list.map((toast) => (
        <div key={toast.id} className="toast" data-tone={toast.tone}>
          <span>{toast.text}</span>
          {toast.action && (
            <button
              className="primary toast-action"
              onClick={() => {
                dismissToast(toast.id);
                toast.action?.run();
              }}
            >
              {toast.action.label}
            </button>
          )}
          <button
            className="icon-button"
            onClick={() => dismissToast(toast.id)}
            aria-label={t('Schließen')}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
