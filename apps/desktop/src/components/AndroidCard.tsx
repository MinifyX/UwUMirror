import { useState } from 'react';
import { api, errorText, type AdbStatus, type Device } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { platform } from '../lib/platform';
import type { Stream } from '../lib/streams';
import { Icon } from './Icon';

type Props = {
  adb: AdbStatus | null;
  devices: Device[];
  /** Why the device list couldn't be read, if it couldn't. */
  error: string | null;
  streams: Stream[];
  onPair: () => void;
  onMirror: (device: Device) => Promise<void>;
  onShow: (id: number) => void;
  /** adb was fetched or chosen: look again. */
  onRefresh: () => void;
};

function deviceName(device: Device): string {
  return device.model ?? device.serial.replace(/\._adb-tls-connect\._tcp\.?$/, '');
}

/** The distro package with adb in it, for systems Google builds none for. */
function adbPackageHint(): string {
  return platform() === 'linux'
    ? t('Installiere das Paket „adb“ (Debian/Ubuntu) oder „android-tools“ (Arch) und suche erneut.')
    : t('Installiere Googles Android Platform-Tools und suche erneut.');
}

/** Android phones adb knows, and how to add one. */
export function AndroidCard({
  adb,
  devices,
  error,
  streams,
  onPair,
  onMirror,
  onShow,
  onRefresh,
}: Props) {
  useLanguage();
  const [starting, setStarting] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);

  const download = async () => {
    setDownloading(true);
    setDownloadError(null);
    try {
      await api.androidDownloadAdb();
      onRefresh();
    } catch (e) {
      setDownloadError(errorText(e));
    } finally {
      setDownloading(false);
    }
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
    <section className="card" aria-labelledby="android-title">
      <header className="card-head">
        <span className="card-icon" aria-hidden>
          <Icon name="android" size={20} />
        </span>
        <div className="card-heading">
          <h2 id="android-title">Android</h2>
          <p className="card-sub">{t('Über kabelloses Debugging oder USB')}</p>
        </div>
        {adb?.path && (
          <button className="primary" onClick={onPair}>
            <Icon name="qr" size={15} /> {t('Handy koppeln')}
          </button>
        )}
      </header>

      {adb === null && <p className="status-line">{t('Sucht adb…')}</p>}

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
                <button className="primary" disabled={downloading} onClick={() => void download()}>
                  <Icon name="download" size={15} />{' '}
                  {downloading ? t('Lädt von Google…') : t('Von Google laden')}
                </button>
              )}
              <button onClick={onRefresh}>{t('Erneut suchen')}</button>
            </div>
            {downloadError && <p className="form-error">{downloadError}</p>}
          </div>
        </div>
      )}

      {adb?.path && (
        <>
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
                        className="primary"
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
        </>
      )}
    </section>
  );
}
