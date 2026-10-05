import { useEffect, useState } from 'react';
import { api, errorText, type CastReceiver, type SendStatus } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { Icon } from './Icon';
import { Modal } from './Modal';
import { showToast } from './Toasts';

type Props = {
  /** What the receiver shows as the sender's name. */
  name: string;
  status: SendStatus | null;
  onClose: () => void;
};

/** How often the receiver list is read while the dialog shows. */
const RECEIVER_POLL_MS = 3000;

/** Whether this computer is sending right now, or about to. */
export function isSending(status: SendStatus | null): boolean {
  return status?.state === 'sending' || status?.state === 'connecting';
}

/**
 * Sending this screen to another computer's UwUMirror (Windows only): an
 * action, not a setting, so it opens from the title bar.
 */
export function SendDialog({ name, status, onClose }: Props) {
  useLanguage();
  const [receivers, setReceivers] = useState<CastReceiver[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState<string | null>(null);
  const busy = isSending(status);

  // Receivers come and go on the network; the list follows.
  useEffect(() => {
    if (busy) return;
    let stopped = false;
    const poll = () =>
      void api
        .castReceivers()
        .then((list) => {
          if (stopped) return;
          setReceivers(list);
          setError(null);
        })
        .catch((e) => !stopped && setError(errorText(e)));
    poll();
    const timer = window.setInterval(poll, RECEIVER_POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [busy]);

  const send = async (receiver: CastReceiver) => {
    setStarting(receiver.id);
    try {
      await api.castSend(receiver.id, name);
    } catch (e) {
      showToast(
        t('An „{name}“ lässt sich nicht senden: {error}', {
          name: receiver.name,
          error: errorText(e),
        }),
        'error',
      );
    } finally {
      setStarting(null);
    }
  };

  const stop = () => void api.castSendStop().catch((e) => showToast(errorText(e), 'error'));

  return (
    <Modal title={t('Diesen Bildschirm senden')} onCancel={onClose}>
      <button className="icon-button dialog-close" onClick={onClose} aria-label={t('Schließen')}>
        <Icon name="close" size={16} />
      </button>
      <p className="dialog-lead">{t('An UwUMirror auf einem anderen Computer')}</p>

      {busy && status ? (
        <div className="send-active">
          <p
            className="status-line"
            data-state={status.state === 'sending' ? 'online' : 'starting'}
          >
            <i className="dot" aria-hidden />
            {status.state === 'sending'
              ? t('Sendet an „{name}“', { name: status.receiver ?? '' })
              : t('Verbindet mit „{name}“…', { name: status.receiver ?? '' })}
          </p>
          {status.state === 'sending' && (
            <p className="device-meta">
              {`${status.width} × ${status.height} · ${status.fps} fps · `}
              {status.hardware ? t('Encoder der Grafikkarte') : t('Encoder von Windows')}
              {!status.audio && ` · ${t('ohne Ton')}`}
            </p>
          )}
          <div className="hint-actions">
            <button className="primary" onClick={stop}>
              <Icon name="stop" size={15} /> {t('Senden beenden')}
            </button>
          </div>
        </div>
      ) : (
        <>
          {error && (
            <p className="status-line" data-state="error">
              <i className="dot" aria-hidden />
              {error}
            </p>
          )}
          {receivers.length === 0 && !error ? (
            <p className="empty-line">
              {t(
                'Noch kein anderer Computer zu sehen. Dort UwUMirror öffnen und „Von anderen Computern empfangen“ einschalten.',
              )}
            </p>
          ) : (
            <ul className="device-list">
              {receivers.map((receiver) => (
                <li key={receiver.id} className="device">
                  <Icon name="monitor" size={16} />
                  <span className="device-name">
                    <b>{receiver.name}</b>
                    <span className="device-meta">
                      {receiver.compatible
                        ? `UwUMirror ${receiver.version}`
                        : t('UwUMirror {version} – zu alt oder zu neu für diesen', {
                            version: receiver.version,
                          })}
                    </span>
                  </span>
                  <button
                    className="primary"
                    disabled={!receiver.compatible || starting !== null}
                    onClick={() => void send(receiver)}
                  >
                    {starting === receiver.id ? t('Startet…') : t('Senden')}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      <p className="card-foot">
        {t(
          'Gesendet wird der Hauptbildschirm mit Mauszeiger und Ton. Währenddessen zeigt Windows einen gelben Rahmen um den Bildschirm.',
        )}
      </p>
    </Modal>
  );
}
