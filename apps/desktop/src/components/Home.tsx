import { useCallback, useEffect, useState } from 'react';
import {
  api,
  errorText,
  type AdbStatus,
  type AirplayStatus,
  type CastStatus,
  type Device,
  type SendStatus,
} from '../lib/api';
import { streamIcon, streamSource } from '../lib/devices';
import { t, useLanguage } from '../lib/i18n';
import { useMiracast } from '../lib/miracast';
import { updateSettings, useSettings } from '../lib/settings';
import type { Stream } from '../lib/streams';
import { AirplayCard } from './AirplayCard';
import { AndroidCard } from './AndroidCard';
import { CastCard } from './CastCard';
import { Icon } from './Icon';
import { NyuScene } from './nyu/scenes';
import { PairDialog } from './PairDialog';
import { SendCard } from './SendCard';

type Props = {
  streams: Stream[];
  receiverName: string;
  /** This computer's name, as other computers show it when it sends. */
  computerName: string;
  airplay: AirplayStatus | null;
  cast: CastStatus | null;
  /** This computer can send its screen (Windows). */
  canSend: boolean;
  sending: SendStatus | null;
  ffmpeg: number | null | undefined;
  onAirplayToggle: (on: boolean) => void;
  onCastToggle: (on: boolean) => void;
  onRecheckFfmpeg: () => Promise<void>;
  onShow: (id: number) => void;
  onStop: (id: number) => void;
  onMirror: (device: Device) => Promise<void>;
};

/** How often the phone list is read while the start page shows. */
const DEVICE_POLL_MS = 3000;

/** The start page: the ways in, and the device that mirrors, if one does. */
export function Home(props: Props) {
  useLanguage();
  const settings = useSettings();
  const miracast = useMiracast();
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
            {streams.length === 0 ? t('Bereit zum Spiegeln') : t('Ein Gerät spiegelt gerade')}
          </h1>
          <p className="home-text">
            {t(
              'iPhones, iPads, Macs, Android-Handys und andere Computer in deinem Netzwerk zeigen ihren Bildschirm hier – ohne Cloud, ohne Konto. Eins nach dem anderen: Wer neu spiegelt, löst das Gerät davor ab.',
            )}
          </p>
        </div>
      </section>

      {streams.length > 0 && (
        <section className="running" aria-label={t('Spiegelt gerade')}>
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
          miracast={miracast}
          miracastEnabled={settings.miracastEnabled}
          onMiracastToggle={(miracastEnabled) => updateSettings({ miracastEnabled })}
          adb={adb}
          devices={devices}
          error={deviceError}
          streams={streams}
          onPair={() => setPairing(true)}
          onMirror={props.onMirror}
          onShow={props.onShow}
          onRefresh={refreshAdb}
        />
        <CastCard
          enabled={settings.castEnabled}
          name={props.receiverName}
          status={props.cast}
          onToggle={props.onCastToggle}
        />
        {props.canSend && <SendCard name={props.computerName} status={props.sending} />}
      </div>

      {pairing && <PairDialog onClose={() => setPairing(false)} />}
    </div>
  );
}
