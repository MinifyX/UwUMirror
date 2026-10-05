import { useState, type ReactNode } from 'react';
import type { AirplayStatus, CastStatus, Device } from '../lib/api';
import { deviceName } from '../lib/android';
import { streamIcon, streamSource } from '../lib/devices';
import { t, useLanguage } from '../lib/i18n';
import { useMiracast } from '../lib/miracast';
import { platform } from '../lib/platform';
import { useSettings } from '../lib/settings';
import type { Stream } from '../lib/streams';
import { Icon, type IconName } from './Icon';
import { NyuScene } from './nyu/scenes';
import type { SettingsSection } from './SettingsDialog';
import { airplayState, castState, miracastState, StatusPill, type ReceiverState } from './Status';

type Props = {
  streams: Stream[];
  receiverName: string;
  airplay: AirplayStatus | null;
  cast: CastStatus | null;
  /** libavcodec's version, null without FFmpeg, undefined while unknown. */
  ffmpeg: number | null | undefined;
  /** Phones paired for wireless debugging that adb sees right now. */
  devices: Device[];
  onShow: (id: number) => void;
  onStop: (id: number) => void;
  onMirror: (device: Device) => Promise<void>;
  onSettings: (section: SettingsSection) => void;
};

/** One way in: what it is for, how in one sentence, and whether it is on. */
function Way({
  icon,
  title,
  via,
  state,
  section,
  onSettings,
  children,
}: {
  icon: IconName;
  title: string;
  via: string;
  state?: ReceiverState;
  section: SettingsSection;
  onSettings: (section: SettingsSection) => void;
  children?: ReactNode;
}) {
  const off = state?.tone === 'off';
  const trouble = state?.tone === 'error';
  return (
    <li className="way">
      <span className="way-icon" aria-hidden>
        <Icon name={icon} size={20} />
      </span>
      <div className="way-body">
        <div className="way-head">
          <h2>{title}</h2>
          <span className="way-via">{via}</span>
          <span className="spacer" />
          {state && <StatusPill state={state} />}
        </div>
        {children}
        {(off || trouble) && (
          <button className="link-button" onClick={() => onSettings(section)}>
            {off ? t('In den Einstellungen einschalten') : t('Mehr dazu in den Einstellungen')}
            <Icon name="chevronRight" size={14} />
          </button>
        )}
      </div>
    </li>
  );
}

/**
 * The start page, kept clean: Nyu, the name devices look for, how each kind
 * of device mirrors here and whether that way is on — and whatever mirrors
 * right now. Every switch lives in the settings.
 */
export function Home(props: Props) {
  useLanguage();
  const settings = useSettings();
  const miracast = useMiracast();
  const [starting, setStarting] = useState<string | null>(null);
  const { streams, receiverName: name, onSettings } = props;
  const windows = platform() === 'windows';
  const hasMiracast = windows && miracast?.state !== 'unsupported';
  const miracastName = miracast?.name || t('dieser Computer');

  const airplay = airplayState(settings.airplayEnabled, props.airplay, name);
  const cast = castState(settings.castEnabled, props.cast, name);
  const mira = miracastState(settings.miracastEnabled, miracast);

  const mirror = async (device: Device) => {
    setStarting(device.serial);
    try {
      await props.onMirror(device);
    } finally {
      setStarting(null);
    }
  };
  const ready = props.devices.filter(
    (device) =>
      device.state === 'device' &&
      !streams.some((s) => s.kind === 'android' && s.address === device.serial),
  );

  return (
    <div className="home">
      <div className="home-inner">
        <section className="home-hero">
          <NyuScene name={streams.length > 0 ? 'connecting' : 'waiting'} className="home-scene" />
          <div className="home-hero-text">
            <h1 className="home-title">
              {streams.length === 0 ? t('Bereit zum Spiegeln') : t('Ein Gerät spiegelt gerade')}
            </h1>
            <p className="home-name">
              {t('Geräte finden diesen Computer als')} <b>{name}</b>
            </p>
            <p className="home-text">
              {t(
                'Im eigenen Netzwerk, ohne Cloud, ohne Konto. Eins nach dem anderen: Wer neu spiegelt, löst das Gerät davor ab.',
              )}
            </p>
          </div>
        </section>

        {streams.length > 0 && (
          <section className="home-section" aria-labelledby="running-title">
            <h2 id="running-title" className="section-label">
              {t('Spiegelt gerade')}
            </h2>
            <ul className="running">
              {streams.map((stream) => (
                <li key={stream.id} className="running-item">
                  <span className="way-icon" aria-hidden>
                    <Icon name={streamIcon(stream)} size={18} />
                  </span>
                  <span className="running-name">
                    <b>{stream.name}</b>
                    <span className="device-meta">
                      {streamSource(stream)}
                      {stream.width > 0 && ` · ${stream.width} × ${stream.height}`}
                    </span>
                  </span>
                  <button className="primary" onClick={() => props.onShow(stream.id)}>
                    {t('Zeigen')}
                  </button>
                  <button onClick={() => props.onStop(stream.id)}>{t('Beenden')}</button>
                </li>
              ))}
            </ul>
          </section>
        )}

        <section className="home-section" aria-labelledby="ways-title">
          <h2 id="ways-title" className="section-label">
            {t('So spiegelst du hierher')}
          </h2>
          <ul className="ways">
            <Way
              icon="phone"
              title={t('iPhone, iPad & Mac')}
              via="AirPlay"
              state={airplay}
              section="airplay"
              onSettings={onSettings}
            >
              <p className="way-how">
                {t('Kontrollzentrum → „Bildschirmsynchronisierung“ → „{name}“.', { name })}
              </p>
              {settings.airplayEnabled && props.ffmpeg === null && (
                <button className="way-note" onClick={() => onSettings('airplay')}>
                  <Icon name="volumeOff" size={14} />
                  {t('Ohne Ton: FFmpeg fehlt')}
                </button>
              )}
            </Way>

            {hasMiracast && (
              <Way
                icon="cast"
                title={t('Android & Windows-PCs')}
                via="Miracast"
                state={mira}
                section="miracast"
                onSettings={onSettings}
              >
                <p className="way-how">
                  {t(
                    'Am Handy „Smart View“, „Bildschirm spiegeln“ oder „Cast“, am PC Win+K – dann „{name}“ wählen.',
                    { name: miracastName },
                  )}
                </p>
                {settings.miracastEnabled && miracast?.pin && (
                  <p className="way-pin">
                    {t('Gib diese PIN auf dem Gerät ein:')} <b>{miracast.pin}</b>
                  </p>
                )}
              </Way>
            )}

            <Way
              icon="android"
              title={hasMiracast ? t('Pixel & andere Android-Handys') : t('Android-Handys')}
              via={t('Kabelloses Debugging')}
              section="android"
              onSettings={onSettings}
            >
              <p className="way-how">
                {hasMiracast
                  ? t('Für Handys ohne Miracast: einmal koppeln, dann hier mit einem Klick.')
                  : t('Einmal koppeln, dann hier mit einem Klick spiegeln.')}
              </p>
              {ready.length > 0 && (
                <ul className="way-devices">
                  {ready.map((device) => (
                    <li key={device.serial}>
                      <Icon name={device.wireless ? 'wifi' : 'usb'} size={15} />
                      <span className="way-device-name">{deviceName(device)}</span>
                      <button
                        className="primary"
                        disabled={starting !== null}
                        onClick={() => void mirror(device)}
                      >
                        {starting === device.serial ? t('Startet…') : t('Spiegeln')}
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <button className="link-button" onClick={() => onSettings('android')}>
                {props.devices.length > 0 ? t('Gekoppelte Handys') : t('Handy koppeln')}
                <Icon name="chevronRight" size={14} />
              </button>
            </Way>

            <Way
              icon="monitor"
              title={t('Andere Computer')}
              via="UwUCast"
              state={cast}
              section="cast"
              onSettings={onSettings}
            >
              <p className="way-how">
                {t('Dort in UwUMirror oben auf „Senden“ und „{name}“ wählen (Windows).', { name })}
              </p>
            </Way>
          </ul>
        </section>
      </div>
    </div>
  );
}
