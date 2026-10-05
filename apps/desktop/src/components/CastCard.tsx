import type { CastStatus } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { Icon } from './Icon';
import { Toggle } from './Toggle';

type Props = {
  enabled: boolean;
  name: string;
  status: CastStatus | null;
  onToggle: (on: boolean) => void;
};

/** Receiving from other computers' UwUMirror: on or off, and how to send from one. */
export function CastCard({ enabled, name, status, onToggle }: Props) {
  useLanguage();
  const running = !!status?.running;
  const state = !enabled ? 'off' : status?.error ? 'error' : running ? 'online' : 'starting';

  return (
    <section className="card" aria-labelledby="cast-title">
      <header className="card-head">
        <span className="card-icon" aria-hidden>
          <Icon name="monitor" size={20} />
        </span>
        <div className="card-heading">
          <h2 id="cast-title">{t('Andere Computer')}</h2>
          <p className="card-sub">{t('Mit UwUMirror, über das Netzwerk')}</p>
        </div>
        <Toggle
          checked={enabled}
          onChange={onToggle}
          label={t('Von anderen Computern empfangen')}
        />
      </header>

      <p className="status-line" data-state={state}>
        <i className="dot" aria-hidden />
        {state === 'off' && t('Aus – andere Computer sehen UwUMirror gerade nicht.')}
        {state === 'starting' && t('Startet…')}
        {state === 'online' && t('Empfangsbereit als „{name}“', { name })}
        {state === 'error' &&
          t('Der Empfang konnte nicht starten: {error}', { error: status?.error ?? '' })}
      </p>

      {enabled && (
        <ol className="steps">
          <li>{t('Auf dem anderen Computer läuft UwUMirror, im selben Netzwerk wie dieser.')}</li>
          <li>
            {t(
              'Windows-PCs: Dort unter „Diesen Bildschirm senden“ auf „{name}“ klicken – fertig.',
              {
                name,
              },
            )}
          </li>
        </ol>
      )}

      {running && (
        <p className="card-foot">
          {t(
            'Taucht UwUMirror nicht auf? Die Firewall muss eingehende Verbindungen erlauben (TCP {port} und mDNS, UDP 5353).',
            { port: status?.port ?? 7100 },
          )}
        </p>
      )}
    </section>
  );
}
