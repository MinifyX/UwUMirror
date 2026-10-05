import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  api,
  errorText,
  onCastSend,
  type AirplayStatus,
  type AppInfo,
  type CastStatus,
  type Device,
  type SendStatus,
} from './lib/api';
import { useAdb, useAdbDevices } from './lib/android';
import { t, useLanguage } from './lib/i18n';
import { applyMiracast } from './lib/miracast';
import { getSettings, RESOLUTIONS, receiverName, useSettings } from './lib/settings';
import { onStreamEnded, onStreamStarted, startStreams, useStreams } from './lib/streams';
import { Home } from './components/Home';
import { Icon } from './components/Icon';
import { PinPrompt } from './components/PinPrompt';
import { SettingsDialog, type SettingsSection } from './components/SettingsDialog';
import { isSending, SendDialog } from './components/SendDialog';
import { StreamView } from './components/StreamView';
import { TitleBar } from './components/TitleBar';
import { showToast, Toasts } from './components/Toasts';

export function App() {
  useLanguage();
  const settings = useSettings();
  const streams = useStreams();
  const [activeId, setActiveId] = useState<number | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState<SettingsSection | null>(null);
  const [sendOpen, setSendOpen] = useState(false);
  const [computer, setComputer] = useState('');
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [airplay, setAirplay] = useState<AirplayStatus | null>(null);
  const [cast, setCast] = useState<CastStatus | null>(null);
  const [sending, setSending] = useState<SendStatus | null>(null);
  const activeRef = useRef(activeId);
  activeRef.current = activeId;
  const backgroundRef = useRef<HTMLDivElement>(null);
  const { adb, refresh: refreshAdb } = useAdb();
  const { devices, error: deviceError } = useAdbDevices(adb);
  const dialogOpen = settingsOpen !== null || sendOpen;

  // Behind an open dialog the window can't be clicked or tabbed into.
  useEffect(() => {
    if (backgroundRef.current) backgroundRef.current.inert = dialogOpen;
  }, [dialogOpen]);

  const name = receiverName(settings, computer);

  const setWindowFullscreen = useCallback((on: boolean) => {
    setFullscreen(on);
    void getCurrentWindow()
      .setFullscreen(on)
      .catch(() => undefined);
  }, []);

  // Streams, video, and what this computer is called.
  useEffect(() => {
    void startStreams().catch((error) => showToast(errorText(error), 'error'));
    void api
      .computerName()
      .then(setComputer)
      .catch(() => undefined);
    void api
      .appInfo()
      .then(setInfo)
      .catch(() => undefined);
  }, []);

  // New streams come to the front if wanted; ended ones leave with a note.
  useEffect(() => {
    const offStart = onStreamStarted((stream) => {
      const { showNewStreams, fullscreenNewStreams } = getSettings();
      if (showNewStreams) {
        setActiveId(stream.id);
        if (fullscreenNewStreams && stream.kind !== 'airplayaudio') setWindowFullscreen(true);
      } else {
        showToast(t('{name} spiegelt jetzt.', { name: stream.name }));
      }
    });
    const offEnd = onStreamEnded((ended) => {
      if (activeRef.current === ended.id) {
        setActiveId(null);
        setWindowFullscreen(false);
      }
      showToast(
        ended.reason
          ? t('{name}: Spiegelung abgebrochen ({reason})', {
              name: ended.name,
              reason: ended.reason,
            })
          : t('{name} hat die Spiegelung beendet.', { name: ended.name }),
        ended.reason ? 'error' : 'info',
      );
    });
    return () => {
      offStart();
      offEnd();
    };
  }, [setWindowFullscreen]);

  // The log follows its setting, from the start on.
  useEffect(() => {
    void api.logDetail(settings.detailedLog).catch(() => undefined);
  }, [settings.detailedLog]);

  // The AirPlay receiver follows the settings.
  useEffect(() => {
    if (settings.airplayEnabled && !computer) return; // the name isn't known yet
    const [width, height] = RESOLUTIONS[settings.airplayResolution];
    let stale = false;
    void api
      .airplayApply({
        enabled: settings.airplayEnabled,
        name,
        width,
        height,
        fps: settings.airplayFps,
        audio: settings.airplayAudio,
      })
      .then((status) => !stale && setAirplay(status))
      .catch(
        (error) =>
          !stale && setAirplay({ running: false, port: null, name, error: errorText(error) }),
      );
    return () => {
      stale = true;
    };
  }, [
    computer,
    name,
    settings.airplayEnabled,
    settings.airplayResolution,
    settings.airplayFps,
    settings.airplayAudio,
  ]);

  // The Miracast receiver (Windows) follows the settings, too.
  useEffect(() => {
    void applyMiracast();
  }, [settings.miracastEnabled, settings.miracastAudio]);

  // Receiving from other computers follows its setting, under the same name.
  useEffect(() => {
    if (settings.castEnabled && !computer) return; // the name isn't known yet
    let stale = false;
    void api
      .castApply({ enabled: settings.castEnabled, name })
      .then((status) => !stale && setCast(status))
      .catch((error) => !stale && setCast({ running: false, port: null, error: errorText(error) }));
    return () => {
      stale = true;
    };
  }, [computer, name, settings.castEnabled]);

  // Sending this screen: its state, and a note when the other side ends it
  // or it breaks off (stopping here needs no note).
  useEffect(() => {
    let receiver: string | null = null;
    void api
      .castSendStatus()
      .then(setSending)
      .catch(() => undefined);
    const off = onCastSend((status) => {
      if (status.ended?.how === 'byReceiver') {
        showToast(t('„{name}“ hat das Senden beendet.', { name: receiver ?? '' }));
      } else if (status.ended?.how === 'failed') {
        showToast(t('Senden abgebrochen: {error}', { error: status.ended.error }), 'error');
      }
      receiver = status.receiver ?? receiver;
      setSending(status);
    });
    return () => void off.then((unlisten) => unlisten());
  }, []);

  // A stream that is gone falls back to the start page.
  useEffect(() => {
    if (activeId !== null && !streams.some((s) => s.id === activeId)) setActiveId(null);
  }, [activeId, streams]);

  const active = streams.find((s) => s.id === activeId) ?? null;

  const stop = useCallback((id: number) => {
    void api.streamStop(id).catch((error) => showToast(errorText(error), 'error'));
  }, []);

  const mirror = useCallback(async (device: Device) => {
    const s = getSettings();
    try {
      await api.androidMirror(device.serial, {
        maxSize: s.androidMaxSize,
        bitRate: s.androidBitRate * 1_000_000,
        maxFps: s.androidFps,
        audio: s.androidAudio,
      });
    } catch (error) {
      showToast(
        t('{name} lässt sich nicht spiegeln: {error}', {
          name: device.model ?? device.serial,
          error: errorText(error),
        }),
        'error',
      );
    }
  }, []);

  // Keyboard: Ctrl+, settings, Ctrl+0 start, Ctrl+1 the stream, F11 full screen.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const ctrl = event.ctrlKey || event.metaKey;
      if (ctrl && event.key === ',') {
        event.preventDefault();
        setSettingsOpen((open) => open ?? 'general');
      } else if (ctrl && (event.key === '0' || event.key === '1')) {
        event.preventDefault();
        if (event.key === '0') setActiveId(null);
        else if (streams[0]) setActiveId(streams[0].id);
      } else if (event.key === 'F11' && active && active.kind !== 'airplayaudio') {
        event.preventDefault();
        setWindowFullscreen(!fullscreen);
      } else if (event.key === 'Escape' && fullscreen) {
        setWindowFullscreen(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [streams, active, fullscreen, setWindowFullscreen]);

  const recheckFfmpeg = async () => {
    const ffmpeg = await api.ffmpegRecheck().catch(() => null);
    setInfo((current) => (current ? { ...current, ffmpeg } : current));
    showToast(
      ffmpeg
        ? t('FFmpeg gefunden – der Ton kommt beim nächsten Spiegeln mit.')
        : t('FFmpeg ist noch nicht zu finden.'),
      ffmpeg ? 'info' : 'error',
    );
  };

  return (
    <div className="shell" data-fullscreen={fullscreen && !!active}>
      <div className="background" ref={backgroundRef}>
        <TitleBar onSettings={() => setSettingsOpen('general')}>
          {info?.castSend && (
            <button
              className="titlebar-send"
              data-active={isSending(sending)}
              onClick={() => setSendOpen(true)}
              title={t('Diesen Bildschirm an einen anderen Computer senden')}
            >
              <Icon name="screenShare" size={16} />
              {isSending(sending)
                ? t('Sendet an „{name}“', { name: sending?.receiver ?? '' })
                : t('Senden')}
            </button>
          )}
        </TitleBar>
        <main className="main">
          {active ? (
            <StreamView
              key={active.id}
              stream={active}
              fullscreen={fullscreen}
              onToggleFullscreen={() => setWindowFullscreen(!fullscreen)}
              onStop={() => stop(active.id)}
              onHome={() => setActiveId(null)}
            />
          ) : (
            <Home
              streams={streams}
              receiverName={name}
              airplay={airplay}
              cast={cast}
              ffmpeg={info === null ? undefined : info.ffmpeg}
              devices={devices}
              onShow={setActiveId}
              onStop={stop}
              onMirror={mirror}
              onSettings={setSettingsOpen}
            />
          )}
        </main>
      </div>
      {settingsOpen && (
        <SettingsDialog
          section={settingsOpen}
          onClose={() => setSettingsOpen(null)}
          computer={computer}
          info={info}
          airplay={airplay}
          cast={cast}
          adb={adb}
          onAdbRefresh={refreshAdb}
          devices={devices}
          deviceError={deviceError}
          streams={streams}
          onMirror={mirror}
          onShow={(id) => {
            setSettingsOpen(null);
            setActiveId(id);
          }}
          onRecheckFfmpeg={recheckFfmpeg}
        />
      )}
      {sendOpen && (
        <SendDialog name={computer || name} status={sending} onClose={() => setSendOpen(false)} />
      )}
      <PinPrompt />
      <Toasts />
    </div>
  );
}
