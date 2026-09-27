import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useRef, useState } from 'react';
import { api, errorText, type AirplayStatus, type AppInfo, type Device } from './lib/api';
import { t, useLanguage } from './lib/i18n';
import {
  getSettings,
  RESOLUTIONS,
  receiverName,
  updateSettings,
  useSettings,
} from './lib/settings';
import { onStreamEnded, onStreamStarted, startStreams, useStreams } from './lib/streams';
import { Home } from './components/Home';
import { SettingsDialog } from './components/SettingsDialog';
import { StreamView } from './components/StreamView';
import { TabBar } from './components/TabBar';
import { TitleBar } from './components/TitleBar';
import { showToast, Toasts } from './components/Toasts';

export function App() {
  useLanguage();
  const settings = useSettings();
  const streams = useStreams();
  const [activeId, setActiveId] = useState<number | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [computer, setComputer] = useState('');
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [airplay, setAirplay] = useState<AirplayStatus | null>(null);
  const activeRef = useRef(activeId);
  activeRef.current = activeId;
  const backgroundRef = useRef<HTMLDivElement>(null);

  // Behind an open dialog the window can't be clicked or tabbed into.
  useEffect(() => {
    if (backgroundRef.current) backgroundRef.current.inert = settingsOpen;
  }, [settingsOpen]);

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

  // A tab whose stream is gone falls back to the start page.
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

  // Keyboard: Ctrl+, settings, Ctrl+0 start, Ctrl+1–9 streams, F11 full screen.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const ctrl = event.ctrlKey || event.metaKey;
      if (ctrl && event.key === ',') {
        event.preventDefault();
        setSettingsOpen(true);
      } else if (ctrl && /^[0-9]$/.test(event.key)) {
        event.preventDefault();
        const index = Number(event.key);
        if (index === 0) setActiveId(null);
        else if (streams[index - 1]) setActiveId(streams[index - 1]!.id);
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
        <TitleBar onSettings={() => setSettingsOpen(true)} />
        <TabBar streams={streams} activeId={activeId} onSelect={setActiveId} onStop={stop} />
        <main className="main">
          {active ? (
            <StreamView
              key={active.id}
              stream={active}
              fullscreen={fullscreen}
              onToggleFullscreen={() => setWindowFullscreen(!fullscreen)}
              onStop={() => stop(active.id)}
            />
          ) : (
            <Home
              streams={streams}
              receiverName={name}
              airplay={airplay}
              ffmpeg={info === null ? undefined : info.ffmpeg}
              onAirplayToggle={(airplayEnabled) => updateSettings({ airplayEnabled })}
              onRecheckFfmpeg={recheckFfmpeg}
              onShow={setActiveId}
              onStop={stop}
              onMirror={mirror}
            />
          )}
        </main>
      </div>
      {settingsOpen && (
        <SettingsDialog onClose={() => setSettingsOpen(false)} computer={computer} info={info} />
      )}
      <Toasts />
    </div>
  );
}
