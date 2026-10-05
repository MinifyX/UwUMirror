import { useEffect, useState } from 'react';
import { api, errorText, type AdbStatus, type Device, type MiracastStatus } from '../lib/api';
import { t, useLanguage } from '../lib/i18n';
import { platform } from '../lib/platform';
import type { Stream } from '../lib/streams';
import { Icon } from './Icon';
import { Toggle } from './Toggle';

type Props = {
  /** The Miracast receiver; null while unknown. */
  miracast: MiracastStatus | null;
  miracastEnabled: boolean;
  onMiracastToggle: (on: boolean) => void;
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

/** The status line's dot: mint when phones can come, pink while starting. */
function miracastTone(status: MiracastStatus | null, enabled: boolean): string {
  if (!enabled) return 'off';
  switch (status?.state) {
    case undefined:
    case 'starting':
      return 'starting';
    case 'listening':
    case 'connected':
      return 'online';
    case 'off':
      return 'off';
    default:
      return 'error';
  }
}

function miracastText(status: MiracastStatus | null, enabled: boolean): string {
  if (!enabled) return t('Aus – Handys und PCs sehen UwUMirror gerade nicht.');
  const name = status?.name || t('dieser Computer');
  switch (status?.state) {
    case 'listening':
      return t('Empfangsbereit als „{name}“', { name });
    case 'connected':
      return t('Verbunden – ein Gerät spiegelt über Miracast.');
    case 'noWifiDirect':
      return t(
        'Dieser Computer kann kein Miracast: Sein WLAN-Adapter (oder dessen Treiber) kann kein Wi-Fi Direct.',
      );
    case 'wifiOff':
      return t(
        'WLAN ist aus. Miracast braucht WLAN an diesem Computer – mit einem Netz verbunden sein muss er nicht.',
      );
    case 'disabledByPolicy':
      return t('Eine Richtlinie verbietet das Projizieren auf diesen PC.');
    case 'busy':
      return t(
        'Windows gibt den Empfang gerade nicht her – projiziert dieser PC selbst auf einen anderen Bildschirm?',
      );
    case 'failed':
      return t('Miracast konnte nicht starten: {error}', { error: status.error ?? '?' });
    default:
      return t('Startet…');
  }
}

/**
 * Android phones (and Windows PCs): Miracast first, where Windows lends its
 * receiver — most phones can cast without any setup. Wireless debugging
 * stays as the advanced way, for Pixels and the like, and the only one on
 * macOS and Linux.
 */
export function AndroidCard({
  miracast,
  miracastEnabled,
  onMiracastToggle,
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
  // On Windows the card is Miracast's while its state is still unknown, too.
  const hasMiracast = platform() === 'windows' && miracast?.state !== 'unsupported';
  // Phones paired before stay in sight: the advanced part opens by itself
  // once there are some.
  const [advanced, setAdvanced] = useState(false);
  const anyDevices = devices.length > 0;
  useEffect(() => {
    if (anyDevices) setAdvanced(true);
  }, [anyDevices]);

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

  const name = miracast?.name || t('dieser Computer');
  const tone = miracastTone(miracast, miracastEnabled);
  const ready = miracastEnabled && (tone === 'online' || tone === 'starting');

  const debugging = (
    <>
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
    </>
  );

  const pairButton = adb?.path && (
    <button className="primary" onClick={onPair}>
      <Icon name="qr" size={15} /> {t('Handy koppeln')}
    </button>
  );

  return (
    <section className="card" aria-labelledby="android-title">
      <header className="card-head">
        <span className="card-icon" aria-hidden>
          <Icon name="android" size={20} />
        </span>
        <div className="card-heading">
          <h2 id="android-title">{hasMiracast ? t('Android & Windows-PCs') : 'Android'}</h2>
          <p className="card-sub">
            {hasMiracast ? 'Miracast' : t('Über kabelloses Debugging oder USB')}
          </p>
        </div>
        {hasMiracast ? (
          <Toggle
            checked={miracastEnabled}
            onChange={onMiracastToggle}
            label={t('Miracast empfangen')}
          />
        ) : (
          pairButton
        )}
      </header>

      {hasMiracast ? (
        <>
          <p className="status-line" data-state={tone}>
            <i className="dot" aria-hidden />
            {miracastText(miracast, miracastEnabled)}
          </p>

          {miracastEnabled && miracast?.pin && (
            <div className="hint" data-tone="warning">
              <Icon name="info" size={16} />
              <div>
                <p className="hint-title">{t('Gib diese PIN auf dem Gerät ein:')}</p>
                <p className="pin">{miracast.pin}</p>
              </div>
            </div>
          )}

          {ready && (
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
          )}

          <details
            className="how-to"
            open={advanced}
            onToggle={(event) => setAdvanced(event.currentTarget.open)}
          >
            <summary>{t('Erweitert: Kabelloses Debugging (z. B. für Pixel)')}</summary>
            <div className="advanced">
              <p className="card-foot">
                {t(
                  'Pixel-Handys können kein Miracast. Mit kabellosem Debugging spiegelt jedes Android-Handy ab Android 11 – einmal koppeln, dann ein Klick.',
                )}
              </p>
              {pairButton && <div className="hint-actions">{pairButton}</div>}
              {debugging}
            </div>
          </details>
        </>
      ) : (
        <>
          {debugging}
          {platform() !== 'windows' && (
            <p className="card-foot">
              {t(
                'Miracast („Smart View“, „Cast“) empfängt UwUMirror nur unter Windows. Hier geht Android über kabelloses Debugging.',
              )}
            </p>
          )}
        </>
      )}
    </section>
  );
}
