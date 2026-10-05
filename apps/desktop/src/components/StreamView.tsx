import { useEffect, useRef, useState } from 'react';
import type { AudioStatus } from '../lib/api';
import { streamIcon, streamSource } from '../lib/devices';
import { t, useLanguage } from '../lib/i18n';
import { platform } from '../lib/platform';
import type { PlayerInfo } from '../lib/player';
import type { Stream } from '../lib/streams';
import { Icon } from './Icon';
import { NyuScene } from './nyu/scenes';

type Props = {
  stream: Stream;
  fullscreen: boolean;
  onToggleFullscreen: () => void;
  onStop: () => void;
  /** Back to the start page; the mirroring goes on. */
  onHome: () => void;
};

const BACKENDS = { webcodecs: 'WebCodecs', mediasource: 'Media Source', none: '–' } as const;

function audioLabel(status: AudioStatus | null): string | null {
  switch (status) {
    case 'playing':
      return t('Ton an');
    case 'noDecoder':
      return t('Kein Ton: FFmpeg fehlt');
    case 'noOutput':
      return t('Kein Ton: kein Ausgabegerät');
    case 'unavailable':
      return t('Kein Ton vom Gerät');
    case 'off':
      return t('Ton aus');
    default:
      return null;
  }
}

/**
 * One stream on a dark stage, as big as fits. The picture is the player's own
 * element (a canvas or a video), moved in here while this view shows — the
 * player keeps decoding while the start page shows.
 */
export function StreamView({ stream, fullscreen, onToggleFullscreen, onStop, onHome }: Props) {
  useLanguage();
  // The button that was clicked would keep focus, and with it the full-screen
  // bar open (it shows while anything in it has focus).
  const toggle = () => {
    (document.activeElement as HTMLElement | null)?.blur();
    onToggleFullscreen();
  };
  const stageRef = useRef<HTMLDivElement>(null);
  const [info, setInfo] = useState<PlayerInfo>(stream.player.current());

  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    stage.prepend(stream.player.element);
    const unsubscribe = stream.player.subscribe(setInfo);
    return () => {
      unsubscribe();
      if (stream.player.element.parentElement === stage) stream.player.element.remove();
    };
  }, [stream.player]);

  const audioOnly = stream.kind === 'airplayaudio';
  const waiting = !audioOnly && info.frames === 0 && !info.error;
  const width = info.width || stream.width;
  const height = info.height || stream.height;
  const audio = audioLabel(stream.audio);

  const bar = (
    <>
      {!fullscreen && (
        <button className="quiet" onClick={onHome} title={t('Start (Strg+0)')}>
          <Icon name="home" size={15} /> {t('Start')}
        </button>
      )}
      <span className="stream-title">
        <Icon name={streamIcon(stream)} size={17} />
        <b>{stream.name}</b>
        <span
          className="meta"
          title={t('Dekodiert mit {backend}', { backend: BACKENDS[info.backend] })}
        >
          {streamSource(stream)}
          {width > 0 && ` · ${width} × ${height}`}
        </span>
      </span>
      {audio && (
        <span className="badge" data-tone={stream.audio === 'playing' ? 'ok' : 'muted'}>
          <Icon name={stream.audio === 'playing' ? 'volume' : 'volumeOff'} size={14} />
          {audio}
        </span>
      )}
      <span className="spacer" />
      {!audioOnly && (
        <button
          className="quiet"
          onClick={toggle}
          title={fullscreen ? t('Vollbild verlassen (F11)') : t('Vollbild (F11)')}
        >
          <Icon name={fullscreen ? 'exitFullscreen' : 'fullscreen'} size={15} />{' '}
          {fullscreen ? t('Vollbild verlassen') : t('Vollbild')}
        </button>
      )}
      <button className="danger" onClick={onStop}>
        <Icon name="stop" size={14} /> {t('Beenden')}
      </button>
    </>
  );

  return (
    <div className="stream" data-fullscreen={fullscreen}>
      {fullscreen ? (
        <div className="fullscreen-bar">{bar}</div>
      ) : (
        <div className="toolbar">{bar}</div>
      )}
      <div
        ref={stageRef}
        className="stage"
        onDoubleClick={audioOnly ? undefined : toggle}
        data-waiting={waiting || audioOnly || !!info.error}
      >
        {audioOnly && (
          <div className="stage-message">
            <NyuScene name="welcome" className="stage-scene" />
            <p className="stage-title">
              {t('{name} spielt Musik über diesen Computer', { name: stream.name })}
            </p>
            <p>{t('Nur Ton, kein Bild. Lautstärke und Titel steuerst du am Gerät.')}</p>
          </div>
        )}
        {waiting && (
          <div className="stage-message">
            <NyuScene name="connecting" className="stage-scene" />
            <p className="stage-title">{t('Warte auf das erste Bild…')}</p>
            <p>{t('Das dauert meist nur einen Moment.')}</p>
          </div>
        )}
        {info.error && (
          <div className="stage-message">
            <NyuScene name="loadError" className="stage-scene" />
            <p className="stage-title">{t('Dieses System kann das Video nicht anzeigen')}</p>
            <p>
              {platform() === 'linux'
                ? t(
                    'Für H.264 braucht die WebView GStreamers Plugins: Pakete gstreamer1.0-libav und gstreamer1.0-plugins-good (Arch: gst-libav, gst-plugins-good).',
                  )
                : t('Der Video-Decoder des Systems hat das Bild abgelehnt.')}
            </p>
          </div>
        )}
        {stream.paused && !waiting && (
          <div className="stage-paused">
            <Icon name="pause" size={16} />
            {t('Pausiert – der Bildschirm ist gesperrt oder die App im Hintergrund.')}
          </div>
        )}
      </div>
    </div>
  );
}
