import { api } from '../../lib/api';
import { t } from '../../lib/i18n';
import { updateSettings, type Settings } from '../../lib/settings';
import { Toggle } from '../Toggle';
import { Row, SectionHead, Segmented } from './Row';

export function General({ settings }: { settings: Settings }) {
  return (
    <>
      <SectionHead title={t('Allgemein')} />
      <Row label={t('Farbschema')}>
        <Segmented
          label={t('Farbschema')}
          value={settings.theme}
          options={[
            ['system', t('System')],
            ['light', t('Hell')],
            ['dark', t('Dunkel')],
          ]}
          onChange={(theme) => updateSettings({ theme })}
        />
      </Row>
      <Row label={t('Sprache')}>
        <Segmented
          label={t('Sprache')}
          value={settings.language}
          options={[
            ['system', t('System')],
            ['de', 'Deutsch'],
            ['en', 'English'],
          ]}
          onChange={(language) => updateSettings({ language })}
        />
      </Row>
      <Row label={t('Animationen')} description={t('Nyu blinzelt und wackelt mit den Ohren.')}>
        <Segmented
          label={t('Animationen')}
          value={settings.motion}
          options={[
            ['system', t('System')],
            ['on', t('An')],
            ['off', t('Aus')],
          ]}
          onChange={(motion) => updateSettings({ motion })}
        />
      </Row>
      <Row
        label={t('Neue Streams sofort zeigen')}
        description={t('Beginnt ein Gerät zu spiegeln, wechselt UwUMirror gleich zu seinem Bild.')}
      >
        <Toggle
          checked={settings.showNewStreams}
          onChange={(showNewStreams) => updateSettings({ showNewStreams })}
          label={t('Neue Streams sofort zeigen')}
        />
      </Row>
      <Row
        label={t('Neue Streams im Vollbild')}
        description={t('Praktisch, wenn dieser Computer am Fernseher oder Beamer hängt.')}
      >
        <Toggle
          checked={settings.fullscreenNewStreams}
          disabled={!settings.showNewStreams}
          onChange={(fullscreenNewStreams) => updateSettings({ fullscreenNewStreams })}
          label={t('Neue Streams im Vollbild')}
        />
      </Row>
      <Row
        label={t('Video-Decoder')}
        description={t(
          'Automatisch nimmt WebCodecs, wo es geht. Zeigt ein Stream kein Bild, hilft vielleicht der andere Weg. Gilt ab dem nächsten Stream.',
        )}
      >
        <Segmented
          label={t('Video-Decoder')}
          value={settings.decoder}
          options={[
            ['auto', t('Automatisch')],
            ['webcodecs', 'WebCodecs'],
            ['mediasource', 'Media Source'],
          ]}
          onChange={(decoder) => updateSettings({ decoder })}
        />
      </Row>
      <Row
        label={t('Ausführliches Protokoll')}
        description={t(
          'Schreibt jeden Schritt mit. Hilft, wenn etwas nicht klappt und du einen Fehler melden willst.',
        )}
      >
        <Toggle
          checked={settings.detailedLog}
          onChange={(detailedLog) => updateSettings({ detailedLog })}
          label={t('Ausführliches Protokoll')}
        />
      </Row>
      <Row
        label={t('Protokoll')}
        description={t(
          'uwumirror.log, und vom Start davor uwumirror.old.log. Es bleibt auf diesem Computer.',
        )}
      >
        <button
          onClick={() =>
            void api.openLogFolder().catch((error) => console.warn('log folder', error))
          }
        >
          {t('Ordner öffnen')}
        </button>
      </Row>
    </>
  );
}
