import { useEffect, useState } from 'react';
import { api, errorText, type AirplayStatus, type CastStatus } from '../../lib/api';
import { t } from '../../lib/i18n';
import { refreshFirewall, setUpFirewall, useFirewall } from '../../lib/firewall';
import { useMiracast } from '../../lib/miracast';
import { platform, systemDoesAirplay } from '../../lib/platform';
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
      <NameRow
        settings={settings}
        computer={computer}
        description={t('So heißt dieser Computer in der Liste auf dem iPhone.')}
      />
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
      <TrustedDevices />

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
      {status?.running && <Firewall port={status.port ?? 7000} />}
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
      {miracast?.state !== 'unsupported' && <FirewallRow />}
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
        description={
          systemDoesAirplay()
            ? t('Offen fürs lokale Netz.')
            : t('Offen fürs lokale Netz, wie AirPlay. Der Name ist derselbe wie bei AirPlay.')
        }
      >
        <Toggle
          checked={settings.castEnabled}
          onChange={(castEnabled) => updateSettings({ castEnabled })}
          label={t('Von anderen Computern empfangen')}
        />
      </Row>
      {systemDoesAirplay() && (
        <NameRow
          settings={settings}
          computer={computer}
          description={t('So heißt dieser Computer, wenn ein anderer hierher senden will.')}
        />
      )}
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
      {status?.running && <Firewall port={status.port ?? 7100} />}
    </>
  );
}

/**
 * Windows' firewall: whether Miracast's picture gets through, and a button
 * that sets it up with one administrator prompt. Nothing elsewhere.
 */
function FirewallRow() {
  const { status, busy } = useFirewall();
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);

  useEffect(() => {
    void refreshFirewall();
  }, []);

  if (!status?.needed) return null;
  const description = status.error
    ? t('Die Firewall lässt sich nicht lesen: {error}', { error: status.error })
    : status.ready
      ? t('Eingerichtet. Miracast-Bilder kommen auch über Wi-Fi Direct herein.')
      : t(
          'Fehlt. Miracast läuft über Wi-Fi Direct, und das zählt für Windows als öffentliches Netzwerk. Einrichten fragt einmal nach Administratorrechten.',
        );
  return (
    <>
      <Row label={t('Firewall')} description={description}>
        {status.ready ? (
          <span className="pill" data-state="online">
            {t('Eingerichtet')}
          </span>
        ) : (
          <button
            data-secondary
            disabled={busy}
            onClick={() => {
              setResult(null);
              void setUpFirewall().then((outcome) =>
                setResult(
                  outcome.ok
                    ? { text: t('Eingerichtet. Miracast darf jetzt herein.'), error: false }
                    : { text: outcome.text, error: !outcome.declined },
                ),
              );
            }}
          >
            {busy ? t('Wartet…') : t('Einrichten')}
          </button>
        )}
      </Row>
      {result && (
        <p className="setting-result" data-tone={result.error ? 'error' : undefined}>
          {result.text}
        </p>
      )}
    </>
  );
}

/** Macs that paired with a PIN, and a way to make them ask again. */
function TrustedDevices() {
  const [count, setCount] = useState<number | null>(null);
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);

  useEffect(() => {
    void api
      .airplayTrustedDevices()
      .then(setCount)
      .catch(() => undefined);
  }, []);

  return (
    <>
      <Row
        label={t('Vertraute Geräte vergessen')}
        description={
          count === null
            ? t('Macs, die einmal eine PIN eingegeben haben, kommen danach ohne PIN herein.')
            : t(
                'Macs, die einmal eine PIN eingegeben haben, kommen danach ohne PIN herein. Gekoppelt: {count}.',
                { count },
              )
        }
      >
        <button
          data-secondary
          disabled={count === 0}
          onClick={() => {
            setResult(null);
            api
              .airplayForgetDevices()
              .then(() => {
                setCount(0);
                setResult({
                  text: t('Vergessen. Jeder Mac fragt beim nächsten Mal wieder nach der PIN.'),
                  error: false,
                });
              })
              .catch((error) => setResult({ text: errorText(error), error: true }));
          }}
        >
          {t('Vergessen')}
        </button>
      </Row>
      {result && (
        <p className="setting-result" data-tone={result.error ? 'error' : undefined}>
          {result.text}
        </p>
      )}
    </>
  );
}

/** The name devices see this computer by: AirPlay's list, other computers'. */
function NameRow({
  settings,
  computer,
  description,
}: {
  settings: Settings;
  computer: string;
  description: string;
}) {
  const [name, setName] = useState(settings.receiverName);
  return (
    <Row label={t('Name')} description={description}>
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
  );
}
