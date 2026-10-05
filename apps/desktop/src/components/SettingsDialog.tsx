import { useState } from 'react';
import type { AdbStatus, AirplayStatus, AppInfo, CastStatus, Device } from '../lib/api';
import { N_, t, useLanguage } from '../lib/i18n';
import { platform } from '../lib/platform';
import { useSettings } from '../lib/settings';
import type { Stream } from '../lib/streams';
import { Icon, type IconName } from './Icon';
import { Modal } from './Modal';
import { About } from './settings/About';
import { Android } from './settings/Android';
import { General } from './settings/General';
import { Airplay, Cast, Miracast } from './settings/Receivers';

export type SettingsSection = 'general' | 'airplay' | 'miracast' | 'android' | 'cast' | 'about';

const SECTIONS: { id: SettingsSection; label: string; icon: IconName; windowsOnly?: boolean }[] = [
  { id: 'general', label: N_('Allgemein'), icon: 'sliders' },
  { id: 'airplay', label: N_('AirPlay'), icon: 'phone' },
  { id: 'miracast', label: N_('Miracast'), icon: 'cast', windowsOnly: true },
  { id: 'android', label: N_('Android (Debugging)'), icon: 'android' },
  { id: 'cast', label: N_('Andere Computer'), icon: 'monitor' },
  { id: 'about', label: N_('Über UwUMirror'), icon: 'info' },
];

type Props = {
  /** The section to open with. */
  section: SettingsSection;
  onClose: () => void;
  computer: string;
  info: AppInfo | null;
  airplay: AirplayStatus | null;
  cast: CastStatus | null;
  adb: AdbStatus | null;
  onAdbRefresh: () => void;
  devices: Device[];
  deviceError: string | null;
  streams: Stream[];
  onMirror: (device: Device) => Promise<void>;
  /** Shows a stream: the settings close for it. */
  onShow: (id: number) => void;
  onRecheckFfmpeg: () => Promise<void>;
};

/**
 * Settings, like UwUMail's: a large dialog with the sections on the left
 * (on top in a narrow window) and the section's rows beside them. Every
 * switch of the app lives here; the start page only shows.
 */
export function SettingsDialog(props: Props) {
  useLanguage();
  const settings = useSettings();
  const [section, setSection] = useState<SettingsSection>(props.section);
  const sections = SECTIONS.filter((item) => !item.windowsOnly || platform() === 'windows');
  return (
    <Modal title={t('Einstellungen')} size="wide" onCancel={props.onClose}>
      <button
        className="icon-button dialog-close"
        onClick={props.onClose}
        aria-label={t('Schließen')}
      >
        <Icon name="close" size={16} />
      </button>
      <div className="settings">
        <nav className="settings-nav" aria-label={t('Bereiche')}>
          {sections.map((item) => (
            <button
              key={item.id}
              aria-current={section === item.id ? 'page' : undefined}
              onClick={() => setSection(item.id)}
            >
              <Icon name={item.icon} size={17} />
              <span>{t(item.label)}</span>
            </button>
          ))}
        </nav>
        <div className="settings-content" key={section}>
          {section === 'general' && <General settings={settings} />}
          {section === 'airplay' && (
            <Airplay
              settings={settings}
              computer={props.computer}
              status={props.airplay}
              ffmpeg={props.info === null ? undefined : props.info.ffmpeg}
              onRecheckFfmpeg={props.onRecheckFfmpeg}
            />
          )}
          {section === 'miracast' && <Miracast settings={settings} />}
          {section === 'android' && (
            <Android
              settings={settings}
              adb={props.adb}
              onRefresh={props.onAdbRefresh}
              devices={props.devices}
              error={props.deviceError}
              streams={props.streams}
              onMirror={props.onMirror}
              onShow={props.onShow}
            />
          )}
          {section === 'cast' && (
            <Cast settings={settings} computer={props.computer} status={props.cast} />
          )}
          {section === 'about' && <About info={props.info} />}
        </div>
      </div>
    </Modal>
  );
}
