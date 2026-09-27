import { useEffect, useState, type ReactNode } from 'react';
import { api, errorText, type AdbStatus, type AppInfo } from '../lib/api';
import { N_, t, useLanguage } from '../lib/i18n';
import { receiverName, updateSettings, useSettings, type Settings } from '../lib/settings';
import { Modal } from './Modal';
import { Nyu } from './nyu/Nyu';
import { Toggle } from './Toggle';

export type SettingsSection = 'general' | 'airplay' | 'android' | 'about';

const SECTIONS: { id: SettingsSection; label: string }[] = [
  { id: 'general', label: N_('Allgemein') },
  { id: 'airplay', label: N_('AirPlay') },
  { id: 'android', label: N_('Android') },
  { id: 'about', label: N_('Über UwUMirror') },
];

type Props = {
  onClose: () => void;
  computer: string;
  info: AppInfo | null;
};

/** One setting: a label, an optional explanation and its control. */
function Row({
  label,
  description,
  children,
}: {
  label: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div className="setting-text">
        <p className="setting-label">{label}</p>
        {description && <p className="setting-description">{description}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

function Segmented<T extends string | number>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly (readonly [T, string])[];
  onChange: (value: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map(([option, text]) => (
        <button
          key={String(option)}
          role="radio"
          aria-checked={value === option}
          onClick={() => onChange(option)}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

function General({ settings }: { settings: Settings }) {
  return (
    <>
      <Row label={t('Sprache')}>
        <Segmented
          label={t('Sprache')}
          value={settings.language}
          options={[
            ['system', t('System')],
            ['de', 'Deutsch'],
            ['en', 'English'],
          ]}
          onChange={(language) => updateSettings({ language })}
        />
      </Row>
      <Row label={t('Farbschema')}>
        <Segmented
          label={t('Farbschema')}
          value={settings.theme}
          options={[
            ['system', t('System')],
            ['light', t('Hell')],
            ['dark', t('Dunkel')],
          ]}
          onChange={(theme) => updateSettings({ theme })}
        />
      </Row>
      <Row label={t('Animationen')} description={t('Nyu blinzelt und wackelt mit den Ohren.')}>
        <Segmented
          label={t('Animationen')}
          value={settings.motion}
          options={[
            ['system', t('System')],
            ['on', t('An')],
            ['off', t('Aus')],
          ]}
          onChange={(motion) => updateSettings({ motion })}
        />
      </Row>
      <Row
        label={t('Video-Decoder')}
        description={t(
          'Automatisch nimmt WebCodecs, wo es geht. Zeigt ein Stream kein Bild, hilft vielleicht der andere Weg. Gilt ab dem nächsten Stream.',
        )}
      >
        <Segmented
          label={t('Video-Decoder')}
          value={settings.decoder}
          options={[
            ['auto', t('Automatisch')],
            ['webcodecs', 'WebCodecs'],
            ['mediasource', 'Media Source'],
          ]}
          onChange={(decoder) => updateSettings({ decoder })}
        />
      </Row>
      <Row
        label={t('Neue Streams sofort zeigen')}
        description={t('Beginnt ein Gerät zu spiegeln, wechselt UwUMirror gleich zu seinem Tab.')}
      >
        <Toggle
          checked={settings.showNewStreams}
          onChange={(showNewStreams) => updateSettings({ showNewStreams })}
          label={t('Neue Streams sofort zeigen')}
        />
      </Row>
      <Row
        label={t('Neue Streams im Vollbild')}
        description={t('Praktisch, wenn dieser Computer am Fernseher oder Beamer hängt.')}
      >
        <Toggle
          checked={settings.fullscreenNewStreams}
          disabled={!settings.showNewStreams}
          onChange={(fullscreenNewStreams) => updateSettings({ fullscreenNewStreams })}
          label={t('Neue Streams im Vollbild')}
        />
      </Row>
      <Row
        label={t('Ausführliches Protokoll')}
        description={t(
          'Schreibt jeden Schritt mit. Hilft, wenn etwas nicht klappt und du einen Fehler melden willst.',
        )}
      >
        <Toggle
          checked={settings.detailedLog}
          onChange={(detailedLog) => updateSettings({ detailedLog })}
          label={t('Ausführliches Protokoll')}
        />
      </Row>
      <Row
        label={t('Protokoll')}
        description={t(
          'uwumirror.log, und vom Start davor uwumirror.old.log. Es bleibt auf diesem Computer.',
        )}
      >
        <button
          onClick={() =>
            void api.openLogFolder().catch((error) => console.warn('log folder', error))
          }
        >
          {t('Ordner öffnen')}
        </button>
      </Row>
    </>
  );
}

function Airplay({ settings, computer }: { settings: Settings; computer: string }) {
  const [name, setName] = useState(settings.receiverName);
  return (
    <>
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
    </>
  );
}

function Android({ settings }: { settings: Settings }) {
  const [adb, setAdb] = useState<AdbStatus | null>(null);
  const [path, setPath] = useState(settings.adbPath);
  const [downloading, setDownloading] = useState(false);
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);

  const refresh = () =>
    void api
      .androidChooseAdb(settings.adbPath || null)
      .then(() => api.androidStatus())
      .then(setAdb)
      .catch(() => undefined);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, [settings.adbPath]);

  return (
    <>
      <Row
        label={t('Größe')}
        description={t('Die längere Seite des Bildes. Kleiner ist flüssiger im WLAN.')}
      >
        <Segmented
          label={t('Größe')}
          value={settings.androidMaxSize}
          options={[
            [1280, '1280'],
            [1920, '1920'],
            [2560, '2560'],
            [0, t('Original')],
          ]}
          onChange={(androidMaxSize) => updateSettings({ androidMaxSize })}
        />
      </Row>
      <Row label={t('Bitrate')} description={t('Mehr sieht schärfer aus und braucht mehr WLAN.')}>
        <Segmented
          label={t('Bitrate')}
          value={settings.androidBitRate}
          options={[
            [4, '4 Mbit/s'],
            [8, '8'],
            [16, '16'],
            [24, '24'],
          ]}
          onChange={(androidBitRate) => updateSettings({ androidBitRate })}
        />
      </Row>
      <Row label={t('Bilder pro Sekunde')}>
        <Segmented
          label={t('Bilder pro Sekunde')}
          value={settings.androidFps}
          options={[
            [30, '30'],
            [60, '60'],
          ]}
          onChange={(androidFps) => updateSettings({ androidFps })}
        />
      </Row>
      <Row
        label={t('Ton')}
        description={t('Ab Android 11. Der Ton spielt dann hier statt auf dem Handy.')}
      >
        <Toggle
          checked={settings.androidAudio}
          onChange={(androidAudio) => updateSettings({ androidAudio })}
          label={t('Ton')}
        />
      </Row>
      <Row
        label="adb"
        description={
          adb?.path
            ? t('{path} ({version})', { path: adb.path, version: adb.version ?? '?' })
            : t('Nicht gefunden.')
        }
      >
        {adb?.canDownload && (
          <button
            disabled={downloading}
            onClick={() => {
              setDownloading(true);
              setResult(null);
              api
                .androidDownloadAdb()
                .then(() => {
                  setResult({ text: t('Googles Platform-Tools sind da.'), error: false });
                  refresh();
                })
                .catch((error) => setResult({ text: errorText(error), error: true }))
                .finally(() => setDownloading(false));
            }}
          >
            {downloading ? t('Lädt…') : adb.own ? t('Neu laden') : t('Von Google laden')}
          </button>
        )}
      </Row>
      {result && (
        <p className="setting-result" data-tone={result.error ? 'error' : undefined}>
          {result.text}
        </p>
      )}
      <Row
        label={t('Eigenes adb')}
        description={t('Pfad zu einem bestimmten adb. Leer lassen, damit UwUMirror selbst sucht.')}
      >
        <form
          className="inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            updateSettings({ adbPath: path.trim() });
          }}
        >
          <input
            className="input"
            value={path}
            spellCheck={false}
            placeholder={t('automatisch')}
            onChange={(event) => setPath(event.target.value)}
            onBlur={() => updateSettings({ adbPath: path.trim() })}
          />
        </form>
      </Row>
    </>
  );
}

function About({ info }: { info: AppInfo | null }) {
  const link = (url: string, text: string) => (
    <button className="link-button" onClick={() => void api.openLink(url)}>
      {text}
    </button>
  );
  return (
    <div className="about">
      <Nyu size={96} />
      <p className="about-name">
        <span>UwU</span>Mirror
      </p>
      <p className="about-version">{info?.version ?? ''}</p>
      <p className="about-text">
        {t(
          'Spiegelt iPhones, iPads, Macs und Android-Handys auf deinen Computer. Im eigenen Netzwerk, ohne Cloud, ohne Konto, ohne Telemetrie.',
        )}
      </p>
      <div className="about-actions">
        {link('https://github.com/MinifyX/UwUMirror', t('Projektseite'))}
        {link('https://github.com/MinifyX/UwUMirror/releases', t('Neue Versionen'))}
      </div>
      <details className="about-notice">
        <summary>{t('Lizenzen und Dank')}</summary>
        <p>{t('UwUMirror ist freie Software unter der GNU AGPL v3.0.')}</p>
        <p>
          {t(
            'AirPlay-Empfang nach dem Vorbild von UxPlay, RPiPlay und shairplay; die FairPlay-Entschlüsselung (playfair) stammt aus UxPlay, GNU GPL v3.',
          )}
        </p>
        <p>
          {t('Android-Spiegelung mit dem Server von scrcpy {version} (Genymobile, Apache-2.0).', {
            version: info?.scrcpy ?? '',
          })}
        </p>
        <p>
          {info?.ffmpeg
            ? t('Ton über FFmpeg (libavcodec {version}) vom System, LGPL.', {
                version: info.ffmpeg,
              })
            : t('Ton über FFmpeg vom System, sobald es installiert ist (LGPL).')}
        </p>
        <p>
          {t(
            'AirPlay, iPhone, iPad und Mac sind Marken von Apple Inc., Android ist eine Marke von Google LLC. UwUMirror hat mit beiden nichts zu tun.',
          )}
        </p>
      </details>
    </div>
  );
}

/** Settings: a section list beside the section's rows. */
export function SettingsDialog({ onClose, computer, info }: Props) {
  useLanguage();
  const settings = useSettings();
  const [section, setSection] = useState<SettingsSection>('general');
  return (
    <Modal title={t('Einstellungen')} size="wide" onCancel={onClose}>
      <button className="icon-button settings-close" onClick={onClose} aria-label={t('Schließen')}>
        ×
      </button>
      <div className="settings">
        <nav className="settings-nav" aria-label={t('Bereiche')}>
          {SECTIONS.map((item) => (
            <button
              key={item.id}
              aria-current={section === item.id ? 'page' : undefined}
              onClick={() => setSection(item.id)}
            >
              {t(item.label)}
            </button>
          ))}
        </nav>
        <div className="settings-content">
          {section === 'general' && <General settings={settings} />}
          {section === 'airplay' && <Airplay settings={settings} computer={computer} />}
          {section === 'android' && <Android settings={settings} />}
          {section === 'about' && <About info={info} />}
        </div>
      </div>
    </Modal>
  );
}
