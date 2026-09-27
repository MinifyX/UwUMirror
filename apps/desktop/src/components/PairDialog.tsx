import { useEffect, useState } from 'react';
import { api, errorText, type QrPairing } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { Modal } from './Modal';
import { NyuScene } from './nyu/scenes';

type Mode = 'qr' | 'code' | 'address';
type Phase =
  { state: 'idle' } | { state: 'busy' } | { state: 'done' } | { state: 'error'; message: string };

/**
 * Pairing a phone for wireless debugging, the three ways Android offers: the
 * QR code (Android Studio's way), the six-digit pairing code, or an address
 * for phones that already listen (`adb tcpip`, or a paired phone's own port).
 */
export function PairDialog({ onClose }: { onClose: () => void }) {
  useLanguage();
  const [mode, setMode] = useState<Mode>('qr');
  const [qr, setQr] = useState<QrPairing | null>(null);
  const [phase, setPhase] = useState<Phase>({ state: 'idle' });
  const [address, setAddress] = useState('');
  const [code, setCode] = useState('');
  /** Bumped for a fresh QR code after one ran out. */
  const [attempt, setAttempt] = useState(0);

  // The QR code waits for its phone for as long as it is shown.
  useEffect(() => {
    if (mode !== 'qr') return;
    let stopped = false;
    const start = async () => {
      const next = await api.androidQr();
      if (stopped) return;
      setQr(next);
      setPhase({ state: 'busy' });
      try {
        await api.androidPairQr(next);
        if (!stopped) setPhase({ state: 'done' });
      } catch (error) {
        const message = errorText(error);
        if (stopped || message === 'cancelled') return;
        setPhase({
          state: 'error',
          message: message.includes('answered')
            ? t('Kein Handy hat den Code gescannt. Einfach noch einmal versuchen.')
            : message,
        });
      }
    };
    void start();
    return () => {
      stopped = true;
      void api.androidPairCancel();
    };
  }, [mode, attempt]);

  const pairWithCode = async () => {
    setPhase({ state: 'busy' });
    try {
      await api.androidPairCode(address, code);
      setPhase({ state: 'done' });
    } catch (error) {
      setPhase({ state: 'error', message: errorText(error) });
    }
  };

  const connect = async () => {
    setPhase({ state: 'busy' });
    try {
      await api.androidConnect(address);
      setPhase({ state: 'done' });
    } catch (error) {
      setPhase({ state: 'error', message: errorText(error) });
    }
  };

  const switchTo = (next: Mode) => {
    setMode(next);
    setPhase({ state: 'idle' });
  };

  const addressValid = /^[\w.:[\]-]+:\d{2,5}$/.test(address.trim());

  return (
    <Modal
      title={t('Handy koppeln')}
      onCancel={onClose}
      footer={
        <>
          <span className="spacer" />
          {phase.state === 'done' ? (
            <button className="primary" onClick={onClose}>
              {t('Fertig')}
            </button>
          ) : (
            <button onClick={onClose}>{t('Abbrechen')}</button>
          )}
        </>
      }
    >
      <div className="segmented" role="radiogroup" aria-label={t('Art der Kopplung')}>
        {(
          [
            ['qr', t('QR-Code')],
            ['code', t('Kopplungscode')],
            ['address', t('Adresse')],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            role="radio"
            aria-checked={mode === value}
            onClick={() => switchTo(value)}
          >
            {label}
          </button>
        ))}
      </div>

      {phase.state === 'done' ? (
        <div className="pair-done">
          <NyuScene name="done" className="pair-scene" />
          <p className="dialog-lead">
            {mode === 'address'
              ? t('Verbunden! Das Handy steht jetzt in der Liste.')
              : t('Gekoppelt! Das Handy taucht gleich in der Liste auf – ab jetzt ganz ohne Code.')}
          </p>
        </div>
      ) : mode === 'qr' ? (
        <div className="pair-qr">
          <div className="qr-frame" aria-label={t('QR-Code zum Koppeln')} role="img">
            {qr ? (
              // Our own SVG, drawn by Rust's qrcode crate from a random name
              // and password: nothing from outside ends up in here.
              <div dangerouslySetInnerHTML={{ __html: qr.svg }} />
            ) : (
              <NyuScene name="pair" />
            )}
          </div>
          <ol className="steps">
            <li>{t('Auf dem Handy: Entwickleroptionen → Kabelloses Debugging → einschalten.')}</li>
            <li>{t('„Gerät über QR-Code koppeln“ antippen.')}</li>
            <li>{t('Diesen Code scannen. Handy und Computer müssen im selben WLAN sein.')}</li>
          </ol>
          {phase.state === 'busy' && qr && (
            <p className="status-line" data-state="starting">
              <i className="dot" aria-hidden />
              {t('Warte auf das Handy…')}
            </p>
          )}
        </div>
      ) : mode === 'code' ? (
        <form
          className="form"
          onSubmit={(event) => {
            event.preventDefault();
            if (addressValid && /^\d{6}$/.test(code.trim())) void pairWithCode();
          }}
        >
          <p className="dialog-lead">
            {t(
              'Auf dem Handy unter „Kabelloses Debugging“ → „Gerät über Kopplungscode koppeln“: Dort stehen eine Adresse und sechs Ziffern.',
            )}
          </p>
          <div className="form-row">
            <label className="field grow">
              <span>{t('IP-Adresse und Port')}</span>
              <input
                value={address}
                onChange={(event) => setAddress(event.target.value)}
                placeholder="192.168.1.23:37215"
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <label className="field">
              <span>{t('Kopplungscode')}</span>
              <input
                value={code}
                onChange={(event) => setCode(event.target.value.replace(/\D/g, '').slice(0, 6))}
                placeholder="123456"
                inputMode="numeric"
                autoComplete="off"
              />
            </label>
          </div>
          <button
            className="primary"
            type="submit"
            disabled={phase.state === 'busy' || !addressValid || code.length !== 6}
          >
            {phase.state === 'busy' ? t('Koppelt…') : t('Koppeln')}
          </button>
        </form>
      ) : (
        <form
          className="form"
          onSubmit={(event) => {
            event.preventDefault();
            if (addressValid) void connect();
          }}
        >
          <p className="dialog-lead">
            {t(
              'Für Handys, die schon gekoppelt sind (die Adresse steht oben unter „Kabelloses Debugging“) oder mit adb tcpip lauschen.',
            )}
          </p>
          <label className="field">
            <span>{t('IP-Adresse und Port')}</span>
            <input
              value={address}
              onChange={(event) => setAddress(event.target.value)}
              placeholder="192.168.1.23:5555"
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <button
            className="primary"
            type="submit"
            disabled={phase.state === 'busy' || !addressValid}
          >
            {phase.state === 'busy' ? t('Verbindet…') : t('Verbinden')}
          </button>
        </form>
      )}

      {phase.state === 'error' && (
        <p className="form-error">
          {phase.message}{' '}
          {mode === 'qr' && (
            <button
              className="link-button"
              onClick={() => {
                setQr(null);
                setAttempt((n) => n + 1);
              }}
            >
              {t('Neuen Code zeigen')}
            </button>
          )}
        </p>
      )}
    </Modal>
  );
}
