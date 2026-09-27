import { useState } from 'react';
import type { AirplayStatus } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { platform } from '../lib/platform';
import { Icon } from './Icon';
import { Toggle } from './Toggle';

type Props = {
  enabled: boolean;
  name: string;
  status: AirplayStatus | null;
  /** libavcodec's version, null without FFmpeg, undefined while unknown. */
  ffmpeg: number | null | undefined;
  onToggle: (on: boolean) => void;
  onRecheckFfmpeg: () => Promise<void>;
};

/** Where FFmpeg comes from, per system, for sound with mirrored iPhones. */
function ffmpegHint(): string {
  switch (platform()) {
    case 'windows':
      return t(
        'Installiere FFmpeg als „shared“-Build, am einfachsten in der Eingabeaufforderung mit winget install Gyan.FFmpeg.Shared. Unter Windows auf ARM: einen „winarm64-shared“-Build und seinen bin-Ordner in den PATH.',
      );
    case 'macos':
      return t('Installiere FFmpeg, zum Beispiel mit Homebrew: brew install ffmpeg');
    default:
      return t(
        'Installiere FFmpeg über deine Paketverwaltung, zum Beispiel sudo apt install ffmpeg oder sudo pacman -S ffmpeg.',
      );
  }
}

/** The AirPlay receiver: on or off, what iPhones see, how to mirror. */
export function AirplayCard({ enabled, name, status, ffmpeg, onToggle, onRecheckFfmpeg }: Props) {
  useLanguage();
  const [checking, setChecking] = useState(false);
  const running = !!status?.running;
  const state = !enabled ? 'off' : status?.error ? 'error' : running ? 'online' : 'starting';

  return (
    <section className="card" aria-labelledby="airplay-title">
      <header className="card-head">
        <span className="card-icon" aria-hidden>
          <Icon name="cast" size={20} />
        </span>
        <div className="card-heading">
          <h2 id="airplay-title">{t('iPhone, iPad & Mac')}</h2>
          <p className="card-sub">AirPlay</p>
        </div>
        <Toggle checked={enabled} onChange={onToggle} label={t('AirPlay-Empfang')} />
      </header>

      <p className="status-line" data-state={state}>
        <i className="dot" aria-hidden />
        {state === 'off' && t('Aus – iPhones sehen UwUMirror gerade nicht.')}
        {state === 'starting' && t('Startet…')}
        {state === 'online' && t('Empfangsbereit als „{name}“', { name })}
        {state === 'error' &&
          t('AirPlay konnte nicht starten: {error}', { error: status?.error ?? '' })}
      </p>

      {enabled && (
        <ol className="steps">
          <li>{t('Das Gerät ist im selben WLAN wie dieser Computer.')}</li>
          <li>{t('Kontrollzentrum öffnen und auf „Bildschirmsynchronisierung“ tippen.')}</li>
          <li>{t('„{name}“ auswählen – fertig.', { name })}</li>
        </ol>
      )}

      {enabled && ffmpeg === null && (
        <div className="hint" data-tone="warning">
          <Icon name="volumeOff" size={16} />
          <div>
            <p className="hint-title">{t('Kein Ton beim Spiegeln')}</p>
            <p>
              {t(
                'iPhones schicken ihren Ton als AAC-ELD, und den dekodiert UwUMirror mit FFmpeg vom System.',
              )}{' '}
              {ffmpegHint()}
            </p>
            <button
              className="link-button"
              disabled={checking}
              onClick={() => {
                setChecking(true);
                void onRecheckFfmpeg().finally(() => setChecking(false));
              }}
            >
              {checking ? t('Sucht…') : t('Erneut nach FFmpeg suchen')}
            </button>
          </div>
        </div>
      )}

      {running && (
        <p className="card-foot">
          {t(
            'Taucht UwUMirror nicht auf? Die Firewall muss eingehende Verbindungen erlauben (TCP {port} und mDNS, UDP 5353).',
            { port: status?.port ?? 7000 },
          )}
        </p>
      )}
    </section>
  );
}
