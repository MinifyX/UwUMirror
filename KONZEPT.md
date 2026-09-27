# UwUMirror — Konzept

> Der Bildschirm vom Handy auf dem Computer. Im eigenen Netzwerk, ohne Cloud,
> ohne Konto, mit Nyu.

|                 |                                                                       |
| --------------- | --------------------------------------------------------------------- |
| **Stand**       | 2026-09-27 · 0.1.0-beta.1                                             |
| **Basis**       | Tauri 2 + React wie UwUSSH und UwURDP; AirPlay und Android in Rust    |
| **Bundle-ID**   | `app.uwumirror.desktop`                                               |
| **Repo**        | [MinifyX/UwUMirror](https://github.com/MinifyX/UwUMirror) · AGPL-3.0  |
| **Maskottchen** | Nyu, diesmal als Handspiegel                                          |
| **Plattformen** | Windows (x64, ARM), macOS (Apple-Chip, Intel), Linux (x64, ARM), Arch |

---

## 1. Ziel

Ein Empfänger, auf den iPhones und Android-Handys ihren Bildschirm spiegeln —
so einfach wie ein Apple TV, auf dem Computer, den man sowieso hat.

Der Markt:

- **AirPlay-Empfänger für Windows** (AirServer, Reflector, LonelyScreen &
  Co.) — kosten Geld, wollen oft ein Konto, manche zeigen Werbung.
- **Android** hat keinen Standard. Jeder Hersteller castet anders, meist nur
  auf die eigenen Fernseher. Google Cast lässt sich ohne Googles Zertifikate
  nicht empfangen, Miracast braucht Wi-Fi Direct und jedes System anders.
- **scrcpy** ist hervorragend, aber ein Terminalprogramm, und kennt kein
  AirPlay. **UxPlay** empfängt AirPlay, ist aber ebenfalls eins für die
  Kommandozeile, unter Windows mühsam.

UwUMirror besetzt die Lücke: **beides in einem Fenster, freundlich, frei,
ohne Cloud.**

**Versprechen in einem Satz:** _Handy an, Spiegeln tippen, fertig._

Nebenbei: Spaßprojekt, fast komplett mit Claude geschrieben, kein Support,
keine Termine. Wer es nutzen oder forken will: gern, AGPL-3.0.

### Nicht-Ziele

- Kein Fernwartungswerkzeug. Wer einen Computer steuern will: UwURDP.
- Keine Aufnahme, kein Streaming ins Internet, keine Cloud.
- Kein eigenes Android-Programm, das man erst installieren muss: adb und
  scrcpys Server reichen.

## 2. Zielgruppe

Leute mit einem Computer am Fernseher oder Beamer, die schnell etwas vom Handy
zeigen wollen: Fotos, ein Video, eine App. Familien mit iPhones _und_
Android-Handys. Also: ich.

## 3. Was es kann (0.1.0-beta.1)

**AirPlay (iPhone, iPad, Mac)**

- Erscheint unter „Bildschirmsynchronisierung“ wie ein Apple TV (Bonjour:
  `_airplay._tcp` und `_raop._tcp`), Name einstellbar, Standard „UwUMirror
  (Computername)“.
- Bildschirmsynchronisierung in H.264 bis 4K und 60 Bilder pro Sekunde, so
  weit der Sender mitmacht. Legacy-Pairing (Ed25519/X25519), FairPlay über
  playfair, AES-CTR fürs Bild, AES-CBC für den Ton.
- Ton über FFmpegs libavcodec vom System (AAC-ELD beim Spiegeln, ALAC/AAC bei
  AirPlay-Audio). Fehlt FFmpeg, sagt die App, wie man es bekommt.
- Mehrere Geräte gleichzeitig, jedes in einem eigenen Tab.
- Kein PIN: Solange der Empfang an ist, darf jedes Gerät im Netz spiegeln —
  wie ein Apple TV auf „Alle im selben Netzwerk“. Ein Schalter auf der
  Startseite macht ihn aus.

**Android**

- Koppeln über kabelloses Debugging (ab Android 11): QR-Code wie in Android
  Studio (`WIFI:T:ADB;S:…;P:…;;`, gefunden per mDNS), Kopplungscode, oder
  Verbinden per Adresse. USB geht auch.
- Gekoppelte Handys erscheinen von selbst in der Liste; ein Klick spiegelt.
- Bild über scrcpys Server 4.1 (H.264 vom Hardware-Encoder des Handys), Ton
  als rohes PCM ab Android 11.
- adb vom System, oder Googles platform-tools auf Knopfdruck (Windows, macOS,
  Linux x64).

**Anzeige**

- Dunkle Bühne, Bild so groß wie es passt. Vollbild mit F11 oder Doppelklick,
  Leiste oben wie bei mstsc. Strg+1–9 wechselt zwischen Streams.
- Neue Streams springen auf Wunsch sofort nach vorn, auch gleich im Vollbild
  (für den Beamer).

**Nicht (noch nicht):** Handy mit Maus und Tastatur steuern, AirPlay-Video
(„AirPlay“-Knopf in Video-Apps, HLS), HEVC, PIN für AirPlay, Aufnahme,
automatische Updates. Mit echten Geräten ist die Beta noch nicht getestet —
nur gegen simulierte Sender, siehe `docs/architecture.md`.

## 4. Architektur

```
┌─ WebView ──────────────────────────────────────┐
│  Startseite, Tabs, Einstellungen                │
│  Player pro Stream: WebCodecs → <canvas>        │
│                oder Media Source → <video>      │
└───────────────┬────────────────────────────────┘
                │ Tauri: Befehle, Events (JSON),
                │ Channel (Video, binär)
┌───────────────┴────────────────────────────────┐
│  Rust                                          │
│  Hub ◄── StreamEvents ──┬── AirPlay-Empfänger  │
│   │                     └── Android (adb+scrcpy)│
│   └── Ton: FFmpeg (dlopen) → cpal              │
└───────────────┬───────────────┬────────────────┘
          Bonjour, RTSP,    adb forward,
          TCP/UDP           scrcpy-Server
                │               │
            iPhone / iPad    Android-Handy
```

**Bild wird nie in Rust dekodiert.** Die WebView kann H.264 selbst — mit der
Hardware, ohne Lizenzfragen für uns. Rust entschlüsselt, setzt Startcodes
(Annex B) und reicht die Pakete über einen Tauri-Channel durch. Der Hub hält
pro Stream alles seit dem letzten Keyframe vor, damit eine neu geladene Seite
sofort ein Bild hat — iPhones schicken bei stillem Bildschirm minutenlang
keinen neuen Keyframe.

**Ton wird in Rust dekodiert und abgespielt** (cpal), mit kurzem Puffer, der
bei Überlänge gekürzt wird: Live geht vor Vollständigkeit.

Details, Protokolle und Tests: [`docs/architecture.md`](docs/architecture.md).

## 5. Sicherheit und Datenschutz

- Keine Telemetrie, keine Konten, keine Server außer denen im eigenen Netz.
- Einzige Downloads: Googles platform-tools, nur auf Knopfdruck, direkt von
  `dl.google.com`.
- Gespeichert werden nur Einstellungen (in der WebView) und der AirPlay-Schlüssel
  des Empfängers (`airplay-identity` im App-Datenordner), damit iPhones ihn
  wiedererkennen.
- Alles, was aus dem Netz kommt, ist begrenzt geprüft: RTSP-Zeilen, Header,
  Körper, Paketgrößen; FairPlay-Modi, bevor playfair sie als Tabellenindex
  nimmt; Pfade beim Entpacken der platform-tools.
- Der AirPlay-Empfang ist offen für das lokale Netz (wie ein Apple TV). Wer das
  nicht will, schaltet ihn aus; ein PIN steht auf der Roadmap.
