import { useState } from 'react';
import { api, errorText, type AdbStatus, type Device } from '../../lib/api';
import { deviceName } from '../../lib/android';
import { t } from '../../lib/i18n';
import { platform } from '../../lib/platform';
import { updateSettings, type Settings } from '../../lib/settings';
import type { Stream } from '../../lib/streams';
import { Icon } from '../Icon';
import { PairDialog } from '../PairDialog';
import { Toggle } from '../Toggle';
import { Row, SectionHead, Segmented } from './Row';

/** The distro package with adb in it, for systems Google builds none for. */
function adbPackageHint(): string {
  return platform() === 'linux'
    ? t('Installiere das Paket „adb“ (Debian/Ubuntu) oder „android-tools“ (Arch) und suche erneut.')
    : t('Installiere Googles Android Platform-Tools und suche erneut.');
}

/**
 * Android over wireless debugging (or USB): pairing, the paired phones, the
 * picture they send, and adb itself.
 */
export function Android({
  settings,
  adb,
  onRefresh: refresh,
  devices,
  error,
  streams,
  onMirror,
  onShow,
}: {
  settings: Settings;
  /** Which adb is used; null while it is looked for. */
  adb: AdbStatus | null;
  /** adb was fetched or chosen: look again. */
  onRefresh: () => void;
  devices: Device[];
  /** Why the device list couldn't be read, if it couldn't. */
  error: string | null;
  streams: Stream[];
  onMirror: (device: Device) => Promise<void>;
  onShow: (id: number) => void;
}) {
  const [path, setPath] = useState(settings.adbPath);
  const [pairing, setPairing] = useState(false);
  const [starting, setStarting] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [result, setResult] = useState<{ text: string; error: boolean } | null>(null);

  const download = () => {
    setDownloading(true);
    setResult(null);
    api
      .androidDownloadAdb()
      .then(() => {
        setResult({ text: t('Googles Platform-Tools sind da.'), error: false });
        refresh();
      })
      .catch((e) => setResult({ text: errorText(e), error: true }))
      .finally(() => setDownloading(false));
  };

  const mirror = async (device: Device) => {
    setStarting(device.serial);
    try {
      await onMirror(device);
    } finally {
      setStarting(null);
    }
  };

  const running = (device: Device) =>
    streams.find((s) => s.kind === 'android' && s.address === device.serial);

  return (
    <>
      <SectionHead title={t('Android (Debugging)')}>
        <p className="section-sub">
          {platform() === 'windows'
            ? t('Für Handys ohne Miracast, wie Pixel: über kabelloses Debugging oder USB')
            : t('Über kabelloses Debugging oder USB')}
        </p>
      </SectionHead>

      {adb === null && <p className="empty-line">{t('Sucht adb…')}</p>}

      {adb && !adb.path && (
        <div className="hint">
          <Icon name="info" size={16} />
          <div>
            <p className="hint-title">{t('adb fehlt noch')}</p>
            <p>
              {t(
                'Android-Handys spiegelt UwUMirror über adb, Googles Werkzeug für Entwickler. Es kommt nicht mit UwUMirror mit, weil dafür Googles eigene Bedingungen gelten.',
              )}{' '}
              {!adb.canDownload && adbPackageHint()}
            </p>
            <div className="hint-actions">
              {adb.canDownload && (
                <button className="primary" disabled={downloading} onClick={download}>
                  <Icon name="download" size={15} />{' '}
                  {downloading ? t('Lädt von Google…') : t('Von Google laden')}
                </button>
              )}
              <button onClick={refresh}>{t('Erneut suchen')}</button>
            </div>
          </div>
        </div>
      )}

      {adb?.path && (
        <div className="panel">
          <div className="panel-head">
            <p className="setting-label">{t('Gekoppelte Handys')}</p>
            <span className="spacer" />
            <button className="primary" onClick={() => setPairing(true)}>
              <Icon name="qr" size={15} /> {t('Handy koppeln')}
            </button>
          </div>
          {error && (
            <p className="status-line" data-state="error">
              <i className="dot" aria-hidden />
              {error}
            </p>
          )}
          {devices.length === 0 && !error ? (
            <p className="empty-line">
              {t('Noch kein Handy da. Einmal koppeln, dann taucht es hier von selbst auf.')}
            </p>
          ) : (
            <ul className="device-list">
              {devices.map((device) => {
                const stream = running(device);
                return (
                  <li key={device.serial} className="device">
                    <Icon name={device.wireless ? 'wifi' : 'usb'} size={16} />
                    <span className="device-name">
                      <b>{deviceName(device)}</b>
                      <span className="device-meta">
                        {device.state === 'device' && (device.wireless ? t('WLAN') : 'USB')}
                        {device.state === 'unauthorized' && t('Auf dem Handy erlauben')}
                        {device.state === 'offline' && t('Nicht erreichbar')}
                        {!['device', 'unauthorized', 'offline'].includes(device.state) &&
                          device.state}
                      </span>
                    </span>
                    {stream ? (
                      <button onClick={() => onShow(stream.id)}>{t('Zeigen')}</button>
                    ) : (
                      <button
                        disabled={device.state !== 'device' || starting !== null}
                        onClick={() => void mirror(device)}
                      >
                        {starting === device.serial ? t('Startet…') : t('Spiegeln')}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
          <details className="how-to">
            <summary>{t('So bereitest du das Handy vor')}</summary>
            <ol className="steps">
              <li>
                {t(
                  'Einstellungen → Über das Telefon → sieben Mal auf „Build-Nummer“ tippen. Das schaltet die Entwickleroptionen frei.',
                )}
              </li>
              <li>
                {t(
                  'Einstellungen → System → Entwickleroptionen → „Kabelloses Debugging“ einschalten (ab Android 11). Ältere Handys: „USB-Debugging“ und ein Kabel.',
                )}
              </li>
              <li>
                {t('Hier auf „Handy koppeln“ und auf dem Handy „Gerät über QR-Code koppeln“.')}
              </li>
            </ol>
          </details>
        </div>
      )}

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
          <button disabled={downloading} onClick={download}>
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
      {platform() !== 'windows' && (
        <p className="card-foot">
          {t(
            'Miracast („Smart View“, „Cast“) empfängt UwUMirror nur unter Windows. Hier geht Android über kabelloses Debugging.',
          )}
        </p>
      )}

      {pairing && <PairDialog onClose={() => setPairing(false)} />}
    </>
  );
}
