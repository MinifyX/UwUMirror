import { api, type AppInfo } from '../../lib/api';
import { t } from '../../lib/i18n';
import { Nyu } from '../nyu/Nyu';

export function About({ info }: { info: AppInfo | null }) {
  const link = (url: string, text: string) => (
    <button onClick={() => void api.openLink(url)}>{text}</button>
  );
  return (
    <div className="about">
      <Nyu size={96} />
      <p className="about-name">
        <span>UwU</span>Mirror
      </p>
      <p className="about-version">{info?.version ?? ''}</p>
      <p className="about-text">
        {t(
          'Spiegelt iPhones, iPads, Macs und Android-Handys auf deinen Computer. Im eigenen Netzwerk, ohne Cloud, ohne Konto, ohne Telemetrie.',
        )}
      </p>
      <dl className="about-facts">
        <dt>{t('Ton')}</dt>
        <dd>
          {info?.ffmpeg
            ? t('FFmpeg (libavcodec {version})', { version: info.ffmpeg })
            : t('FFmpeg fehlt')}
        </dd>
        <dt>Android</dt>
        <dd>{t('scrcpy-Server {version}', { version: info?.scrcpy ?? '' })}</dd>
      </dl>
      <div className="about-actions">
        {link('https://github.com/MinifyX/UwUMirror', t('Projektseite'))}
        {link('https://github.com/MinifyX/UwUMirror/releases', t('Neue Versionen'))}
      </div>
      <details className="about-notice">
        <summary>{t('Lizenzen und Dank')}</summary>
        <p>{t('UwUMirror ist freie Software unter der GNU AGPL v3.0.')}</p>
        <p>
          {t(
            'AirPlay-Empfang nach dem Vorbild von UxPlay, RPiPlay und shairplay; die FairPlay-Entschlüsselung (playfair) stammt aus UxPlay, GNU GPL v3.',
          )}
        </p>
        <p>
          {t('Android-Spiegelung mit dem Server von scrcpy {version} (Genymobile, Apache-2.0).', {
            version: info?.scrcpy ?? '',
          })}
        </p>
        <p>
          {info?.ffmpeg
            ? t('Ton über FFmpeg (libavcodec {version}) vom System, LGPL.', {
                version: info.ffmpeg,
              })
            : t('Ton über FFmpeg vom System, sobald es installiert ist (LGPL).')}
        </p>
        <p>
          {t(
            'Schrift: UwU Sans, nach Atkinson Hyperlegible Next (Braille Institute), SIL Open Font License 1.1.',
          )}
        </p>
        <p>
          {t(
            'AirPlay, iPhone, iPad und Mac sind Marken von Apple Inc., Android ist eine Marke von Google LLC. UwUMirror hat mit beiden nichts zu tun.',
          )}
        </p>
      </details>
    </div>
  );
}
