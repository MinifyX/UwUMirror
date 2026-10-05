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
- Ein Gerät nach dem anderen: Wer neu spiegelt, löst das Gerät davor ab.
- Kein PIN: Solange der Empfang an ist, darf jedes Gerät im Netz spiegeln —
  wie ein Apple TV auf „Alle im selben Netzwerk“. Ein Schalter auf der
  Startseite macht ihn aus.
- PIN nur, wenn der Sender sie verlangt (Macs ab macOS Sequoia, verwaltete
  Geräte): UwUMirror zeigt vier Ziffern groß an, gekoppelt wird mit Apples
  SRP-6a (`pair-setup-pin`). Danach kennt UwUMirror den Mac
  (`airplay-trusted`) und fragt nicht wieder; „Vertraute Geräte vergessen“ in
  den Einstellungen setzt das zurück.

**Miracast (Android-Handys, Windows-PCs; nur unter Windows)**

- Der Hauptweg für Android: Am Handy „Smart View“, „Bildschirm spiegeln“ oder
  „Cast“ öffnen und den Computer wählen; Windows-PCs mit Win+K. Kein
  Koppeln, keine Entwickleroptionen. Pixel-Handys können kein Miracast.
- UwUMirror leiht sich den Miracast-Empfänger von Windows
  (`Windows.Media.Miracast`), solange es läuft (Schalter „Miracast
  empfangen“). Der Name ist der von Windows (der Computername).
- Das Bild kommt hier ausnahmsweise dekodiert an (Media Foundation auf der
  Grafikkarte), als NV12 über WebView2s Shared Buffers in die Seite; der Ton
  spielt Windows selbst.

**Android (kabelloses Debugging, überall; unter Windows „Erweitert“)**

- Koppeln über kabelloses Debugging (ab Android 11): QR-Code wie in Android
  Studio (`WIFI:T:ADB;S:…;P:…;;`, gefunden per mDNS), Kopplungscode, oder
  Verbinden per Adresse. USB geht auch.
- Gekoppelte Handys erscheinen von selbst in der Liste; ein Klick spiegelt.
- Bild über scrcpys Server 4.1 (H.264 vom Hardware-Encoder des Handys), Ton
  als rohes PCM ab Android 11.
- adb vom System, oder Googles platform-tools auf Knopfdruck (Windows, macOS,
  Linux x64).

**Andere Computer (UwUCast)**

- Ein Windows-PC mit UwUMirror schickt seinen Bildschirm samt Ton an
  UwUMirror auf einem anderen Computer (Windows, macOS, Linux): „Diesen
  Bildschirm senden“, Empfänger anklicken, fertig. Kein eigenes Programm.
- Eigenes kleines Protokoll (UwUCast, `_uwumirror._tcp` per mDNS, TCP 7100):
  H.264 in Annex B und rohes PCM, wie von AirPlay und Android — der Empfänger
  zeigt es wie jeden anderen Stream.
- Bild über Windows.Graphics.Capture (Hauptbildschirm mit Mauszeiger), H.264
  vom Encoder der Grafikkarte über Media Foundation (oder dem von Windows),
  bis 1080p mit 60 bzw. 30 Bildern; Ton über WASAPI-Loopback. Kein eigener
  Codec.
- Empfang mit eigenem Schalter („Von anderen Computern empfangen“), offen fürs
  lokale Netz wie AirPlay.

**Anzeige**

- Dunkle Bühne, Bild so groß wie es passt. Vollbild mit F11 oder Doppelklick,
  Leiste oben wie bei mstsc. Strg+1–9 wechselt zwischen Streams.
- Neue Streams springen auf Wunsch sofort nach vorn, auch gleich im Vollbild
  (für den Beamer).

**Nicht (noch nicht):** Handy mit Maus und Tastatur steuern, AirPlay-Video
(„AirPlay“-Knopf in Video-Apps, HLS), HEVC, PIN für jeden AirPlay-Sender, Aufnahme,
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
│   │                     ├── Android (adb+scrcpy)│
│   │                     └── UwUCast-Empfänger  │
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

Ausnahme Miracast (Windows): Windows liefert nur fertige Bilder; die gehen als
NV12 über WebView2s Shared Buffers direkt in einen `VideoFrame` der Seite.

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
  nicht will, schaltet ihn aus; ein PIN für jeden Sender steht auf der
  Roadmap (bisher nur für Sender, die selbst danach fragen). Für den Empfang
  von anderen Computern (UwUCast) gilt dasselbe, mit eigenem Schalter; jede
  Länge auf der Leitung ist begrenzt geprüft.
