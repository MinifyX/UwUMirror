import { useCallback, useEffect, useState } from 'react';
import { api, errorText, type AdbStatus, type AirplayStatus, type Device } from '../lib/api';
import { streamIcon, streamSource } from '../lib/devices';
import { t, useLanguage } from '../lib/i18n';
import { useSettings } from '../lib/settings';
import type { Stream } from '../lib/streams';
import { AirplayCard } from './AirplayCard';
import { AndroidCard } from './AndroidCard';
import { Icon } from './Icon';
import { NyuScene } from './nyu/scenes';
import { PairDialog } from './PairDialog';

type Props = {
  streams: Stream[];
  receiverName: string;
  airplay: AirplayStatus | null;
  ffmpeg: number | null | undefined;
  onAirplayToggle: (on: boolean) => void;
  onRecheckFfmpeg: () => Promise<void>;
  onShow: (id: number) => void;
  onStop: (id: number) => void;
  onMirror: (device: Device) => Promise<void>;
};

/** How often the phone list is read while the start page shows. */
const DEVICE_POLL_MS = 3000;

/** The start page: the two ways in, and what is running. */
export function Home(props: Props) {
  useLanguage();
  const settings = useSettings();
  const [adb, setAdb] = useState<AdbStatus | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [pairing, setPairing] = useState(false);

  const refreshAdb = useCallback(() => {
    void api
      .androidChooseAdb(settings.adbPath || null)
      .then(() => api.androidStatus())
      .then(setAdb)
      .catch(() => setAdb({ path: null, version: null, canDownload: false, own: false }));
  }, [settings.adbPath]);

  useEffect(refreshAdb, [refreshAdb]);

  useEffect(() => {
    if (!adb?.path) return;
    let stopped = false;
    const poll = () =>
      void api
        .androidDevices()
        .then((list) => {
          if (stopped) return;
          setDevices(list);
          setDeviceError(null);
        })
        .catch((error) => !stopped && setDeviceError(errorText(error)));
    poll();
    const timer = window.setInterval(poll, DEVICE_POLL_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [adb?.path]);

  const { streams } = props;
  return (
    <div className="home">
      <section className="home-hero">
        <NyuScene name={streams.length > 0 ? 'connecting' : 'waiting'} className="home-scene" />
        <div>
          <h1 className="home-title">
            {streams.length === 0
              ? t('Bereit zum Spiegeln')
              : streams.length === 1
                ? t('Ein Gerät spiegelt gerade')
                : t('{count} Geräte spiegeln gerade', { count: streams.length })}
          </h1>
          <p className="home-text">
            {t(
              'iPhones, iPads, Macs und Android-Handys in deinem Netzwerk zeigen ihren Bildschirm hier – ohne Cloud, ohne Konto.',
            )}
          </p>
        </div>
      </section>

      {streams.length > 0 && (
        <section className="running" aria-label={t('Laufende Streams')}>
          {streams.map((stream) => (
            <div key={stream.id} className="running-item">
              <Icon name={streamIcon(stream)} size={18} />
              <span className="running-name">
                <b>{stream.name}</b>
                <span className="device-meta">
                  {streamSource(stream)}
                  {stream.width > 0 && ` · ${stream.width} × ${stream.height}`}
                </span>
              </span>
              <button onClick={() => props.onShow(stream.id)}>{t('Zeigen')}</button>
              <button className="quiet" onClick={() => props.onStop(stream.id)}>
                {t('Beenden')}
              </button>
            </div>
          ))}
        </section>
      )}

      <div className="cards">
        <AirplayCard
          enabled={settings.airplayEnabled}
          name={props.receiverName}
          status={props.airplay}
          ffmpeg={props.ffmpeg}
          onToggle={props.onAirplayToggle}
          onRecheckFfmpeg={props.onRecheckFfmpeg}
        />
        <AndroidCard
          adb={adb}
          devices={devices}
          error={deviceError}
          streams={streams}
          onPair={() => setPairing(true)}
          onMirror={props.onMirror}
          onShow={props.onShow}
          onRefresh={refreshAdb}
        />
      </div>

      {pairing && <PairDialog onClose={() => setPairing(false)} />}
    </div>
  );
}
