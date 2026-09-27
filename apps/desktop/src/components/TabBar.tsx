import { streamIcon } from '../lib/devices';
import { t, useLanguage } from '../lib/i18n';
import type { Stream } from '../lib/streams';
import { Icon } from './Icon';

type Props = {
  streams: Stream[];
  /** A stream id, or null for the start page. */
  activeId: number | null;
  onSelect: (id: number | null) => void;
  onStop: (id: number) => void;
};

/**
 * The start page and one tab per stream. A tab lives as long as its stream:
 * closing the tab ends the mirroring, and a stream the device ends takes its
 * tab with it. Middle click closes, like in a browser.
 */
export function TabBar({ streams, activeId, onSelect, onStop }: Props) {
  useLanguage();
  return (
    <div className="tabbar">
      <div className="tabs" role="tablist" aria-label={t('Streams')}>
        <div className="tab" data-active={activeId === null}>
          <button
            role="tab"
            className="tab-select"
            aria-selected={activeId === null}
            onClick={() => onSelect(null)}
            title={t('Start (Strg+0)')}
          >
            <span className="tab-icon" aria-hidden>
              <Icon name="home" size={15} />
            </span>
            <span className="tab-title">{t('Start')}</span>
          </button>
        </div>
        {streams.map((stream, index) => {
          const active = stream.id === activeId;
          return (
            <div
              key={stream.id}
              className="tab"
              data-active={active}
              onMouseDown={(event) => {
                if (event.button === 1) {
                  event.preventDefault();
                  onStop(stream.id);
                }
              }}
            >
              <button
                role="tab"
                className="tab-select"
                aria-selected={active}
                title={
                  index < 9
                    ? t('{name} (Strg+{number})', { name: stream.name, number: index + 1 })
                    : stream.name
                }
                onClick={() => onSelect(stream.id)}
              >
                <span className="tab-icon" aria-hidden>
                  <Icon name={streamIcon(stream)} size={15} />
                  <i className="dot" data-state={stream.paused ? 'idle' : 'online'} />
                </span>
                <span className="tab-title">{stream.name}</span>
              </button>
              <button
                className="tab-close"
                onClick={() => onStop(stream.id)}
                title={t('Spiegelung beenden')}
                aria-label={t('{name}: Spiegelung beenden', { name: stream.name })}
              >
                ×
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
