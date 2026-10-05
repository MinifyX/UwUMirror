import { useState } from 'react';
import type { AirplayStatus, CastStatus } from '../../lib/api';
import { t } from '../../lib/i18n';
import { useMiracast } from '../../lib/miracast';
import { platform } from '../../lib/platform';
import { receiverName, updateSettings, type Settings } from '../../lib/settings';
import { Icon } from '../Icon';
import { airplayState, castState, miracastState, StatusLine } from '../Status';
import { Toggle } from '../Toggle';
import { Row, SectionHead, Segmented } from './Row';

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

function Firewall({ port }: { port: number }) {
  return (
    <p className="card-foot">
      {t(
        'Taucht UwUMirror nicht auf? Die Firewall muss eingehende Verbindungen erlauben (TCP {port} und mDNS, UDP 5353).',
        { port },
      )}
    </p>
  );
}

export function Airplay({
  settings,
  computer,
  status,
  ffmpeg,
  onRecheckFfmpeg,
}: {
  settings: Settings;
  computer: string;
  status: AirplayStatus | null;
  /** libavcodec's version, null without FFmpeg, undefined while unknown. */
  ffmpeg: number | null | undefined;
  onRecheckFfmpeg: () => Promise<void>;
}) {
  const [name, setName] = useState(settings.receiverName);
  const [checking, setChecking] = useState(false);
  const shown = receiverName(settings, computer);
  return (
    <>
      <SectionHead title={t('AirPlay')}>
        <p className="section-sub">{t('Für iPhone, iPad und Mac')}</p>
      </SectionHead>
      <StatusLine state={airplayState(settings.airplayEnabled, status, shown)} />
      <Row
        label={t('AirPlay-Empfang')}
        description={t('Solange er an ist, darf jedes Gerät in deinem Netzwerk hierher spiegeln.')}
      >
        <Toggle
          checked={settings.airplayEnabled}
          onChange={(airplayEnabled) => updateSettings({ airplayEnabled })}
          label={t('AirPlay-Empfang')}
        />
      </Row>
      <Row
        label={t('Name')}
        description={t('So heißt dieser Computer in der Liste auf dem iPhone.')}
      >
        <form
          className="inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            updateSettings({ receiverName: name });
          }}
        >
          <input
            className="input"
            value={name}
            maxLength={60}
            placeholder={receiverName({ ...settings, receiverName: '' }, computer)}
            onChange={(event) => setName(event.target.value)}
            onBlur={() => updateSettings({ receiverName: name })}
          />
        </form>
      </Row>
      <Row
        label={t('Auflösung')}
        description={t(
          'Die Größe, um die UwUMirror bittet. Das Gerät schickt höchstens seine eigene.',
        )}
      >
        <Segmented
          label={t('Auflösung')}
          value={settings.airplayResolution}
          options={[
            ['720p', '720p'],
            ['1080p', '1080p'],
            ['1440p', '1440p'],
            ['2160p', '4K'],
          ]}
          onChange={(airplayResolution) => updateSettings({ airplayResolution })}
        />
      </Row>
      <Row label={t('Bilder pro Sekunde')}>
        <Segmented
          label={t('Bilder pro Sekunde')}
          value={settings.airplayFps}
          options={[
            [30, '30'],
            [60, '60'],
          ]}
          onChange={(airplayFps) => updateSettings({ airplayFps })}
        />
      </Row>
      <Row
        label={t('Ton')}
        description={t('Spielt den Ton des Geräts hier ab. Beim Spiegeln braucht das FFmpeg.')}
      >
        <Toggle
          checked={settings.airplayAudio}
          onChange={(airplayAudio) => updateSettings({ airplayAudio })}
          label={t('Ton')}
        />
      </Row>

      {ffmpeg === null && (
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

      <details className="how-to">
        <summary>{t('So spiegelt ein iPhone hierher')}</summary>
        <ol className="steps">
          <li>{t('Das Gerät ist im selben WLAN wie dieser Computer.')}</li>
          <li>{t('Kontrollzentrum öffnen und auf „Bildschirmsynchronisierung“ tippen.')}</li>
          <li>{t('„{name}“ auswählen – fertig.', { name: shown })}</li>
        </ol>
      </details>
      <Firewall port={status?.port ?? 7000} />
    </>
  );
}

export function Miracast({ settings }: { settings: Settings }) {
  const miracast = useMiracast();
  const name = miracast?.name || t('dieser Computer');
  return (
    <>
      <SectionHead title="Miracast">
        <p className="section-sub">{t('Für Android-Handys und Windows-PCs')}</p>
      </SectionHead>
      {miracast?.state === 'unsupported' ? (
        <p className="empty-line">{t('Dieses Windows kann kein Miracast empfangen.')}</p>
      ) : (
        <StatusLine state={miracastState(settings.miracastEnabled, miracast)} />
      )}
      {settings.miracastEnabled && miracast?.pin && (
        <div className="hint" data-tone="warning">
          <Icon name="info" size={16} />
          <div>
            <p className="hint-title">{t('Gib diese PIN auf dem Gerät ein:')}</p>
            <p className="pin">{miracast.pin}</p>
          </div>
        </div>
      )}
      <Row
        label={t('Miracast empfangen')}
        description={t(
          'UwUMirror leiht sich dafür den Miracast-Empfänger von Windows, solange es läuft. Der Name ist der des Computers.',
        )}
      >
        <Toggle
          checked={settings.miracastEnabled}
          onChange={(miracastEnabled) => updateSettings({ miracastEnabled })}
          label={t('Miracast empfangen')}
        />
      </Row>
      <Row
        label={t('Ton über Miracast')}
        description={t(
          'Spielt den Ton von Geräten, die über Miracast spiegeln, hier ab. Größe und Bitrate wählt bei Miracast das Gerät selbst.',
        )}
      >
        <Toggle
          checked={settings.miracastAudio}
          onChange={(miracastAudio) => updateSettings({ miracastAudio })}
          label={t('Ton über Miracast')}
        />
      </Row>
      <details className="how-to">
        <summary>{t('So spiegelt ein Handy hierher')}</summary>
        <ol className="steps">
          <li>
            {t(
              'Am Handy „Smart View“, „Bildschirm spiegeln“ oder „Cast“ öffnen (Samsung, Xiaomi, OnePlus, Huawei und viele mehr) und „{name}“ wählen.',
              { name },
            )}
          </li>
          <li>{t('Windows-PCs: Win+K drücken und „{name}“ wählen.', { name })}</li>
          <li>
            {t(
              'Beide brauchen nur WLAN – im selben Netz müssen sie nicht sein. Das erste Mal fragt Windows vielleicht nach der Firewall: erlauben.',
            )}
          </li>
        </ol>
      </details>
      <p className="card-foot">
        {t(
          'Pixel-Handys können kein Miracast. Mit kabellosem Debugging spiegelt jedes Android-Handy ab Android 11 – einmal koppeln, dann ein Klick.',
        )}
      </p>
    </>
  );
}

export function Cast({
  settings,
  computer,
  status,
}: {
  settings: Settings;
  computer: string;
  status: CastStatus | null;
}) {
  const name = receiverName(settings, computer);
  return (
    <>
      <SectionHead title={t('Andere Computer')}>
        <p className="section-sub">{t('UwUMirror auf einem anderen Computer sendet hierher')}</p>
      </SectionHead>
      <StatusLine state={castState(settings.castEnabled, status, name)} />
      <Row
        label={t('Von anderen Computern empfangen')}
        description={t(
          'Offen fürs lokale Netz, wie AirPlay. Der Name ist derselbe wie bei AirPlay.',
        )}
      >
        <Toggle
          checked={settings.castEnabled}
          onChange={(castEnabled) => updateSettings({ castEnabled })}
          label={t('Von anderen Computern empfangen')}
        />
      </Row>
      <details className="how-to">
        <summary>{t('So sendet ein anderer Computer hierher')}</summary>
        <ol className="steps">
          <li>{t('Auf dem anderen Computer läuft UwUMirror, im selben Netzwerk wie dieser.')}</li>
          <li>
            {t('Windows-PCs: Dort oben auf „Senden“ klicken und „{name}“ wählen – fertig.', {
              name,
            })}
          </li>
        </ol>
      </details>
      <Firewall port={status?.port ?? 7100} />
    </>
  );
}
