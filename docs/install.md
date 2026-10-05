# Installing UwUMirror

[Deutsch weiter unten](#uwumirror-installieren)

UwUMirror runs on **Windows 10 and 11** (x64 and ARM), **macOS 11 or newer**
(Apple silicon and Intel) and **Linux** (x86_64 and arm64). It receives what
an iPhone, iPad or Mac sends over **AirPlay**, and mirrors an **Android** phone
over wireless debugging or USB — all in your own network, no cloud, no
account. On Windows and macOS the setup installs for your user only — no admin
rights.

It is a first beta. Every download is built and checked by CI, but not every
system has been tried by hand yet, so expect rough edges.

Windows and macOS get UwUMirror's own setup with Nyu in it, Linux a package
for your distribution or a portable folder. Download from the
[releases](https://github.com/MinifyX/UwUMirror/releases): take the newest one
at the top. Betas are marked **Pre-release**. The file names carry no version,
so `https://github.com/MinifyX/UwUMirror/releases/latest/download/<file>`
always gets the newest finished one.

| System                     | File under **Assets**                                                  |
| -------------------------- | ---------------------------------------------------------------------- |
| Windows 10/11 (x64)        | `UwUMirror-windows-x64-setup.exe`                                      |
| Windows 11 on ARM          | `UwUMirror-windows-arm64-setup.exe`                                    |
| macOS (Intel & Apple chip) | `UwUMirror-macos-universal.dmg`                                        |
| Ubuntu / Debian            | `UwUMirror-linux-x64.deb` · ARM: `UwUMirror-linux-arm64.deb`           |
| Fedora / openSUSE          | `UwUMirror-linux-x64.rpm` · ARM: `UwUMirror-linux-arm64.rpm`           |
| Arch Linux                 | `PKGBUILD` → `makepkg -si` (the AUR package follows)                   |
| Linux, portable            | `UwUMirror-linux-x64-portable.tar.gz` · ARM: `…-arm64-portable.tar.gz` |

**Checking the download (optional).** Each release has a `SHA256SUMS.txt`. On
macOS and Linux: `shasum -a 256 -c SHA256SUMS.txt --ignore-missing` in the
download folder. On Windows, in PowerShell:
`Get-FileHash "$env:USERPROFILE\Downloads\UwUMirror-windows-x64-setup.exe"`
and compare with the line in the file.

## What UwUMirror uses from your system

UwUMirror itself is one program. A few things it takes from the system when
they are there, and tells you in the app when they are not:

- **Sound for AirPlay** needs FFmpeg's `libavcodec`. Apple sends mirrored
  sound as AAC-ELD; the one standalone decoder for it (Fraunhofer's FDK)
  can't be combined with the AGPL, so UwUMirror uses FFmpeg's (LGPL). On
  Windows and macOS it brings a tiny build of its own, with nothing but the
  AAC and ALAC decoders: nothing to install. The Linux packages depend on
  your distribution's FFmpeg and install it along. An FFmpeg of your own
  (4 to 9) is used when the bundled one is missing.
- **Android** needs `adb`. On Windows, macOS and Linux x64 UwUMirror offers to
  download Google's platform-tools for you. On Linux arm64 Google has none:
  install your distribution's `adb` / `android-tools` package. An `adb` you
  already have (Android Studio, Homebrew, a package) is found too.
- **Video on Linux** is decoded by WebKitGTK through GStreamer: H.264 needs
  `gst-libav` (Debian/Ubuntu: `gstreamer1.0-libav`) and `gst-plugins-good`.
  The .deb and the AUR package depend on them; with the .rpm and the portable
  folder, install them yourself (see below). Windows and macOS have what they
  need built in.

## Windows

Double-click the setup. Windows will most likely show **"Windows protected your
PC"**: the setup isn't signed with a paid code-signing certificate, so
SmartScreen doesn't know it yet. Click **More info**, then **Run anyway**. Your
browser may also say the file is "not commonly downloaded"; keep it anyway (in
Edge: `…` → **Keep** → **Show more** → **Keep anyway**).

- **Install** sets everything up in a few seconds.
- **Options** lets you change the folder (default
  `%LOCALAPPDATA%\Programs\UwUMirror`) or turn off the desktop shortcut.
- If Microsoft Edge WebView2 is missing (Windows 11 always has it), the setup
  offers to download and install it.

**The firewall.** The first time UwUMirror starts, Windows asks whether it may
communicate on networks. Tick **Private networks** (that is enough, and
"Public" is not needed) and click **Allow**. Your home network has to be set
to _Private_ for this to count: **Settings → Network & internet → Wi-Fi (or
Ethernet) → your network → Network profile type → Private network**. Clicked
it away? **Windows Security → Firewall & network protection → Allow an app
through firewall → Change settings**, then tick **Private** next to UwUMirror.

Uninstall from **Windows Settings → Apps → Installed apps → UwUMirror**. The
firewall entry Windows made stays in that list until you remove it there.

## macOS

Open the `.dmg` and double-click **UwUMirror Setup**. UwUMirror isn't notarized
by Apple (that needs a paid developer account), so the first time macOS says
it can't check the app. Then:

1. Open **System Settings → Privacy & Security**.
2. Scroll down: next to "UwUMirror Setup was blocked", click **Open Anyway**
   and confirm.

(On macOS 14 and older, right-clicking the setup and choosing **Open** works
too.) The setup installs **UwUMirror** into `/Applications`, or into
`~/Applications` if your user may not write to `/Applications`. The installed
app starts without that question.

When UwUMirror starts, macOS may ask whether it may **find devices on your
local network** and, if the firewall is on, whether it may **accept incoming
connections**. Allow both, or phones won't see it. Both can be changed later
under **System Settings → Privacy & Security → Local Network** and **System
Settings → Network → Firewall → Options**.

A Mac has an AirPlay receiver of its own (**System Settings → General →
AirDrop & Handoff → AirPlay Receiver**). UwUMirror runs next to it and takes
the next free port; turning the Mac's own one off just avoids two entries
with similar names on your phone.

To uninstall, run the setup again and choose **Uninstall …** — it asks whether
to keep your settings and the AirPlay key. Dragging the app to the Trash works
too, but leaves them in
`~/Library/Application Support/app.uwumirror.desktop`.

## Linux

**Ubuntu, Debian and relatives:** `sudo apt install ./UwUMirror-linux-x64.deb`
(`…-arm64.deb` on ARM). apt installs what sound and picture need along with
it — `gstreamer1.0-libav` (which brings FFmpeg's libraries) and
`gstreamer1.0-plugins-good` — and the recommended `adb` and `ffmpeg` unless
you switched recommends off. Uninstall with `sudo apt remove uwumirror`.

**Fedora, openSUSE:** `sudo dnf install ./UwUMirror-linux-x64.rpm` or
`sudo zypper install ./UwUMirror-linux-x64.rpm`. The extras come separately:
`android-tools` for Android, and FFmpeg and GStreamer's libav plugin for sound
and video. Fedora's own `ffmpeg-free` leaves out codecs for patent reasons;
the full `ffmpeg` and `gstreamer1-plugin-libav` from
[RPM Fusion](https://rpmfusion.org/) are the safe choice (openSUSE: from
Packman). Uninstall with `sudo dnf remove uwumirror` or
`sudo zypper remove uwumirror`.

**Arch Linux:** download the release's `PKGBUILD` into an empty folder and
run `makepkg -si` there; it repacks the `.deb` and checks it against the
release's checksums. (`uwumirror-bin` on the AUR follows.) `ffmpeg`,
`gst-libav` and `gst-plugins-good` come along; `android-tools` is optional.

The packages install the app system-wide as package `uwumirror`
(`/usr/bin/uwumirror-desktop`), with a menu entry, using the system's
WebKitGTK 4.1 and ALSA (`libasound2`, on Arch `alsa-lib`) for sound.

**Portable:** unpack `UwUMirror-linux-x64-portable.tar.gz` anywhere and start
`./UwUMirror/uwumirror`. It brings its own WebKit and installs nothing. Sound,
AirPlay's decoder, `adb` and the H.264 plugins still come from the system, as
above.

**Android over USB** needs permission for your user to talk to the phone:
Debian and Ubuntu's `adb` package brings the udev rules
(`android-sdk-platform-tools-common`), on Arch it's `android-udev`. Wireless
debugging needs nothing of the sort.

**The firewall.** Most desktop distributions ship with the firewall off or
open for the home network; Fedora Workstation's default zone already allows
what UwUMirror needs. With **ufw** on (`sudo ufw status`), allow your own
network — adjust the address to yours:

```sh
sudo ufw allow from 192.168.1.0/24 comment 'UwUMirror and the local network'
```

With **firewalld** and a stricter zone, put your network into the `home` zone
or allow it outright:

```sh
sudo firewall-cmd --permanent --zone=home --add-source=192.168.1.0/24
sudo firewall-cmd --permanent --zone=home --add-service=mdns
sudo firewall-cmd --reload
```

## What goes through the network

UwUMirror receives; your phone sends. So, unlike most apps, it has to accept
incoming connections — from your own network only, nothing goes to or comes
from the internet:

| What                          | Port                                           |
| ----------------------------- | ---------------------------------------------- |
| AirPlay                       | TCP 7000, or the next free one up to 7009      |
| AirPlay stream, timing, sound | TCP and UDP, ports picked per session          |
| Finding each other (mDNS)     | UDP 5353, multicast in the local network       |
| Android                       | outgoing only: UwUMirror connects to the phone |

Because some ports are only picked when a session starts, a firewall rule for
7000 alone isn't enough: allow the program (Windows, macOS) or your own
network (Linux), as above. Guest networks, "client isolation" on the Wi-Fi and
VPNs that take all traffic keep phone and computer apart — both have to be in
the same network, and see each other.

## First steps

- **iPhone or iPad:** open Control Center, tap **Screen Mirroring** and pick
  this computer. For sound only — music, podcasts — use the AirPlay button in
  the app and pick it there.
- **Mac:** Control Center → **Screen Mirroring** → this computer.
- **Android, wireless (Android 11 or newer):** on the phone, turn on the
  developer options (**Settings → About phone**, tap **Build number** seven
  times), then **Developer options → Wireless debugging** → on →
  **Pair device with QR code**, and scan the code UwUMirror shows. Phone and
  computer need to be in the same Wi-Fi.
- **Android over USB:** turn on **USB debugging** in the developer options,
  plug the phone in, and confirm **Allow USB debugging?** on the phone.
- The phone's sound comes along on **Android 11 and newer**; older phones
  mirror the picture only.

## Updates

UwUMirror 0.1 doesn't update itself yet. For a new version, run the new setup
over the installed UwUMirror, or install the new package over the old one; on
Arch, build the new release's `PKGBUILD`. Settings and the AirPlay
key stay. The portable folder is replaced by unpacking the new one.

## Where your data lives

UwUMirror keeps very little: its settings, and the AirPlay key — the one that
lets your Apple devices recognise this computer as the same receiver next
time. Nothing you mirror is recorded or stored.

| What                         | Windows                                 | macOS                                                 | Linux                                  |
| ---------------------------- | --------------------------------------- | ----------------------------------------------------- | -------------------------------------- |
| Settings and the AirPlay key | `%APPDATA%\app.uwumirror.desktop\`      | `~/Library/Application Support/app.uwumirror.desktop` | `~/.local/share/app.uwumirror.desktop` |
| The window's own web data    | `%LOCALAPPDATA%\app.uwumirror.desktop\` | `~/Library/WebKit/app.uwumirror.desktop`              | `~/.local/share/app.uwumirror.desktop` |
| The program                  | `%LOCALAPPDATA%\Programs\UwUMirror\`    | `/Applications/UwUMirror.app`                         | `/usr/bin/uwumirror-desktop`           |

Google's platform-tools, if UwUMirror downloaded them for you, live in
UwUMirror's data folders too, and go with them. Deleting the AirPlay key is
harmless: UwUMirror makes a new one, and your devices simply see a new
receiver.

## If something goes wrong

- **The phone doesn't list UwUMirror:** almost always the network. Check the
  firewall (above), that both are in the same network (not a guest network),
  and that the Wi-Fi doesn't isolate its clients. On Windows the network has
  to be _Private_.
- **Picture, but no sound (AirPlay):** UwUMirror says in the app whether
  FFmpeg's `libavcodec` is missing (on Linux with the .rpm or the portable
  folder, see [What UwUMirror uses from your system](#what-uwumirror-uses-from-your-system)).
  If it isn't, switch on the detailed log in the settings, mirror for a
  minute with something playing, and attach the log to an issue: it says
  whether sound arrived, decoded and was louder than silence.
- **No picture on Linux, or a black one:** the GStreamer H.264 plugins are
  missing — install `gstreamer1.0-libav` / `gst-libav`.
- **"adb not found":** let UwUMirror download platform-tools, or install your
  system's `adb` / `android-tools`.
- **Android says "unauthorized", or pairing fails:** unlock the phone and
  accept the prompt on it; for wireless debugging, pair again with a fresh QR
  code.
- **"WebView2 couldn't be installed"** (Windows): install the Evergreen WebView2
  Runtime from [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/)
  and run the setup again.
- **An antivirus program blocks the setup**: that is the same missing
  certificate as SmartScreen's warning. The source of every release is in this
  repository, and the checksums tell you the file is the published one.
- **macOS says the app is damaged**: that happens when the quarantine mark
  survives on the installed app, which the setup avoids. Running
  `xattr -dr com.apple.quarantine /Applications/UwUMirror.app` clears it.
- **Windows on ARM** has its own setup (`UwUMirror-windows-arm64-setup.exe`);
  the x64 one runs there too, emulated and slower.
- Something else? [Open an issue](https://github.com/MinifyX/UwUMirror/issues),
  best with the log: **Settings → General → Detailed log** on, try again,
  then **Log → Open folder** and attach `uwumirror.log`.

Building it yourself instead: [Development](../README.md#development).

---

# UwUMirror installieren

UwUMirror läuft unter **Windows 10 und 11** (x64 und ARM), **macOS 11 oder
neuer** (Apple-Chip und Intel) und **Linux** (x86_64 und arm64). Es empfängt,
was ein iPhone, iPad oder Mac über **AirPlay** schickt, und spiegelt ein
**Android**-Handy über kabelloses Debugging oder USB — alles im eigenen
Netzwerk, ohne Cloud und ohne Konto. Unter Windows und macOS installiert das
Setup nur für deinen Benutzer — ohne Adminrechte.

Es ist eine erste Beta. Jeder Download wird von der CI gebaut und geprüft, von
Hand ausprobiert ist aber noch nicht jedes System — also mit Ecken und Kanten
rechnen.

Windows und macOS bekommen UwUMirrors eigenes Setup mit Nyu, Linux ein Paket
für deine Distribution oder einen portablen Ordner. Lade von den
[Releases](https://github.com/MinifyX/UwUMirror/releases) herunter: das neueste
ganz oben. Betas sind als **Pre-release** markiert. Die Dateinamen enthalten
keine Version, `https://github.com/MinifyX/UwUMirror/releases/latest/download/<Datei>`
holt also immer die neueste fertige.

| System                     | Datei unter **Assets**                                                 |
| -------------------------- | ---------------------------------------------------------------------- |
| Windows 10/11 (x64)        | `UwUMirror-windows-x64-setup.exe`                                      |
| Windows 11 auf ARM         | `UwUMirror-windows-arm64-setup.exe`                                    |
| macOS (Intel & Apple-Chip) | `UwUMirror-macos-universal.dmg`                                        |
| Ubuntu / Debian            | `UwUMirror-linux-x64.deb` · ARM: `UwUMirror-linux-arm64.deb`           |
| Fedora / openSUSE          | `UwUMirror-linux-x64.rpm` · ARM: `UwUMirror-linux-arm64.rpm`           |
| Arch Linux                 | `PKGBUILD` → `makepkg -si` (das AUR-Paket folgt)                       |
| Linux, portabel            | `UwUMirror-linux-x64-portable.tar.gz` · ARM: `…-arm64-portable.tar.gz` |

**Download prüfen (optional).** Jedes Release hat eine `SHA256SUMS.txt`. Unter
macOS und Linux im Download-Ordner: `shasum -a 256 -c SHA256SUMS.txt
--ignore-missing`. Unter Windows in PowerShell:
`Get-FileHash "$env:USERPROFILE\Downloads\UwUMirror-windows-x64-setup.exe"` und
mit der Zeile in der Datei vergleichen.

## Was UwUMirror vom System nutzt

UwUMirror selbst ist ein Programm. Ein paar Dinge nimmt es vom System, wenn sie
da sind, und sagt in der App Bescheid, wenn nicht:

- **Ton über AirPlay** braucht FFmpegs `libavcodec`. Apple schickt den Ton
  beim Spiegeln als AAC-ELD; der einzige eigenständige Decoder dafür
  (Fraunhofers FDK) verträgt sich nicht mit der AGPL, darum nutzt UwUMirror
  den von FFmpeg (LGPL). Unter Windows und macOS bringt es einen winzigen
  eigenen Build mit, nur mit den Decodern für AAC und ALAC: nichts zu
  installieren. Die Linux-Pakete hängen vom FFmpeg deiner Distribution ab und
  installieren es mit. Ein eigenes FFmpeg (4 bis 9) wird genommen, wenn das
  mitgebrachte fehlt.
- **Android** braucht `adb`. Unter Windows, macOS und Linux x64 bietet
  UwUMirror an, Googles platform-tools für dich herunterzuladen. Für Linux
  arm64 hat Google keine: installiere das Paket `adb` / `android-tools` deiner
  Distribution. Ein `adb`, das du schon hast (Android Studio, Homebrew, ein
  Paket), wird auch gefunden.
- **Video unter Linux** dekodiert WebKitGTK über GStreamer: H.264 braucht
  `gst-libav` (Debian/Ubuntu: `gstreamer1.0-libav`) und `gst-plugins-good`.
  Die .deb und das AUR-Paket hängen davon ab; bei .rpm und portablem Ordner
  installierst du sie selbst (siehe unten). Windows und macOS haben alles
  Nötige an Bord.

## Windows

Doppelklick auf das Setup. Windows zeigt sehr wahrscheinlich **„Der Computer
wurde durch Windows geschützt“**: Das Setup ist nicht mit einem
kostenpflichtigen Code-Signing-Zertifikat signiert, deshalb kennt SmartScreen es
noch nicht. Klick auf **Weitere Informationen**, dann auf **Trotzdem
ausführen**. Der Browser meldet vielleicht, die Datei werde „nicht häufig
heruntergeladen“; behalte sie trotzdem (in Edge: `…` → **Beibehalten** → **Mehr
anzeigen** → **Trotzdem beibehalten**).

- **Installieren** richtet alles in ein paar Sekunden ein.
- Unter **Optionen** änderst du den Ordner (Standard
  `%LOCALAPPDATA%\Programs\UwUMirror`) oder schaltest die Desktop-Verknüpfung
  ab.
- Fehlt Microsoft Edge WebView2 (Windows 11 hat es immer), bietet das Setup an,
  es herunterzuladen und zu installieren.

**Die Firewall.** Beim ersten Start fragt Windows, ob UwUMirror in Netzwerken
kommunizieren darf. Hak **Private Netzwerke** an (das reicht, „Öffentlich“
braucht es nicht) und klick auf **Zugriff zulassen**. Dein Heimnetz muss dafür
als _Privat_ eingestellt sein: **Einstellungen → Netzwerk und Internet → WLAN
(oder Ethernet) → dein Netzwerk → Netzwerkprofiltyp → Privates Netzwerk**.
Weggeklickt? **Windows-Sicherheit → Firewall & Netzwerkschutz → Zugriff von
App durch Firewall zulassen → Einstellungen ändern**, dann bei UwUMirror
**Privat** anhaken.

Deinstallieren über **Windows-Einstellungen → Apps → Installierte Apps →
UwUMirror**. Der Firewall-Eintrag, den Windows angelegt hat, bleibt in der
Liste oben, bis du ihn dort entfernst.

## macOS

Die `.dmg` öffnen und **UwUMirror Setup** doppelklicken. UwUMirror ist nicht
bei Apple notarisiert (das braucht einen kostenpflichtigen Entwickler-Account),
deshalb sagt macOS beim ersten Mal, es könne die App nicht prüfen. Dann:

1. **Systemeinstellungen → Datenschutz & Sicherheit** öffnen.
2. Nach unten scrollen: neben „UwUMirror Setup wurde blockiert“ auf **Trotzdem
   öffnen** klicken und bestätigen.

(Unter macOS 14 und älter geht auch Rechtsklick auf das Setup → **Öffnen**.) Das
Setup installiert **UwUMirror** nach `/Applications`, oder nach
`~/Applications`, wenn dein Benutzer nicht in `/Applications` schreiben darf.
Die installierte App startet ohne diese Rückfrage.

Beim Start fragt macOS vielleicht, ob UwUMirror **Geräte im lokalen Netzwerk
finden** darf und, bei eingeschalteter Firewall, ob es **eingehende
Verbindungen annehmen** darf. Beides erlauben, sonst sehen die Handys es
nicht. Später änderbar unter **Systemeinstellungen → Datenschutz & Sicherheit
→ Lokales Netzwerk** und **Systemeinstellungen → Netzwerk → Firewall →
Optionen**.

Ein Mac hat einen eigenen AirPlay-Empfänger (**Systemeinstellungen →
Allgemein → AirDrop & Handoff → AirPlay-Empfänger**). UwUMirror läuft daneben
und nimmt den nächsten freien Port; den eigenen abzuschalten erspart nur zwei
ähnlich benannte Einträge auf dem Handy.

Deinstallieren: das Setup noch einmal starten und **Deinstallieren …** wählen —
es fragt, ob Einstellungen und AirPlay-Schlüssel bleiben sollen. Die App in den
Papierkorb ziehen geht auch, lässt beides aber in
`~/Library/Application Support/app.uwumirror.desktop` liegen.

## Linux

**Ubuntu, Debian und Verwandte:** `sudo apt install ./UwUMirror-linux-x64.deb`
(`…-arm64.deb` auf ARM). apt installiert mit, was Ton und Bild brauchen —
`gstreamer1.0-libav` (bringt FFmpegs Bibliotheken mit) und
`gstreamer1.0-plugins-good` —, dazu die empfohlenen `adb` und `ffmpeg`, außer
du hast Empfehlungen abgeschaltet. Deinstallieren mit `sudo apt remove uwumirror`.

**Fedora, openSUSE:** `sudo dnf install ./UwUMirror-linux-x64.rpm` oder
`sudo zypper install ./UwUMirror-linux-x64.rpm`. Die Extras kommen getrennt:
`android-tools` für Android, FFmpeg und GStreamers libav-Plugin für Ton und
Bild. Fedoras eigenes `ffmpeg-free` lässt Codecs aus Patentgründen weg; das
volle `ffmpeg` und `gstreamer1-plugin-libav` aus
[RPM Fusion](https://rpmfusion.org/) sind die sichere Wahl (openSUSE: aus
Packman). Deinstallieren mit `sudo dnf remove uwumirror` bzw.
`sudo zypper remove uwumirror`.

**Arch Linux:** das `PKGBUILD` des Releases in einen leeren Ordner laden und
dort `makepkg -si` ausführen; es packt die `.deb` um und prüft sie gegen die
Prüfsummen des Releases. (`uwumirror-bin` im AUR folgt.) `ffmpeg`,
`gst-libav` und `gst-plugins-good` kommen mit; `android-tools` ist optional.

Die Pakete installieren die App systemweit als Paket `uwumirror`
(`/usr/bin/uwumirror-desktop`), mit Eintrag im Anwendungsmenü, und nutzen das
WebKitGTK 4.1 des Systems und ALSA (`libasound2`, unter Arch `alsa-lib`) für
den Ton.

**Portabel:** `UwUMirror-linux-x64-portable.tar.gz` irgendwo entpacken und
`./UwUMirror/uwumirror` starten. Bringt sein eigenes WebKit mit und installiert
nichts. Ton, der Decoder für AirPlay, `adb` und die H.264-Plugins kommen
trotzdem vom System, wie oben.

**Android per USB** braucht die Erlaubnis, dass dein Benutzer mit dem Handy
sprechen darf: Das Paket `adb` von Debian und Ubuntu bringt die udev-Regeln mit
(`android-sdk-platform-tools-common`), unter Arch ist es `android-udev`.
Kabelloses Debugging braucht so etwas nicht.

**Die Firewall.** Die meisten Desktop-Distributionen haben die Firewall aus
oder fürs Heimnetz offen; die Standardzone von Fedora Workstation erlaubt schon,
was UwUMirror braucht. Ist **ufw** an (`sudo ufw status`), erlaube dein eigenes
Netz — die Adresse an deins anpassen:

```sh
sudo ufw allow from 192.168.1.0/24 comment 'UwUMirror and the local network'
```

Mit **firewalld** und einer strengeren Zone steck dein Netz in die Zone `home`
oder erlaube es direkt:

```sh
sudo firewall-cmd --permanent --zone=home --add-source=192.168.1.0/24
sudo firewall-cmd --permanent --zone=home --add-service=mdns
sudo firewall-cmd --reload
```

## Was durchs Netzwerk geht

UwUMirror empfängt, dein Handy sendet. Anders als die meisten Apps muss es also
eingehende Verbindungen annehmen — nur aus deinem eigenen Netz, nichts geht ins
Internet oder kommt von dort:

| Was                           | Port                                              |
| ----------------------------- | ------------------------------------------------- |
| AirPlay                       | TCP 7000, oder der nächste freie bis 7009         |
| AirPlay-Datenstrom, Takt, Ton | TCP und UDP, Ports je Sitzung gewählt             |
| Einander finden (mDNS)        | UDP 5353, Multicast im lokalen Netz               |
| Android                       | nur ausgehend: UwUMirror verbindet sich zum Handy |

Weil manche Ports erst beim Start einer Sitzung feststehen, reicht eine
Firewall-Regel nur für 7000 nicht: das Programm erlauben (Windows, macOS) oder
dein eigenes Netz (Linux), wie oben. Gastnetze, „Client-Isolierung“ im WLAN
und VPNs, die allen Verkehr übernehmen, halten Handy und Rechner auseinander —
beide müssen im selben Netz sein und sich sehen können.

## Erste Schritte

- **iPhone oder iPad:** Kontrollzentrum öffnen, auf
  **Bildschirmsynchronisierung** tippen und diesen Rechner wählen. Nur für Ton
  — Musik, Podcasts — den AirPlay-Knopf in der App nehmen und ihn dort wählen.
- **Mac:** Kontrollzentrum → **Bildschirmsynchronisierung** → dieser Rechner.
- **Android, kabellos (ab Android 11):** auf dem Handy die Entwickleroptionen
  einschalten (**Einstellungen → Über das Telefon**, siebenmal auf
  **Build-Nummer** tippen), dann **Entwickleroptionen → Kabelloses Debugging**
  → an → **Gerät über QR-Code koppeln**, und den Code scannen, den UwUMirror
  zeigt. Handy und Rechner müssen im selben WLAN sein.
- **Android per USB:** in den Entwickleroptionen **USB-Debugging** einschalten,
  das Handy anstecken und auf dem Handy **USB-Debugging zulassen?** bestätigen.
- Der Ton des Handys kommt ab **Android 11** mit; ältere Handys spiegeln nur
  das Bild.

## Updates

UwUMirror 0.1 aktualisiert sich noch nicht selbst. Für eine neue Version das
neue Setup über das installierte UwUMirror laufen lassen, oder das neue Paket
über das alte installieren; unter Arch das `PKGBUILD` des neuen
Releases bauen. Einstellungen und AirPlay-Schlüssel bleiben. Den
portablen Ordner ersetzt du, indem du den neuen entpackst.

## Wo deine Daten liegen

UwUMirror merkt sich sehr wenig: seine Einstellungen und den AirPlay-Schlüssel
— den, an dem deine Apple-Geräte diesen Rechner beim nächsten Mal als
denselben Empfänger wiedererkennen. Nichts, was du spiegelst, wird
aufgezeichnet oder gespeichert.

| Was                                 | Windows                                 | macOS                                                 | Linux                                  |
| ----------------------------------- | --------------------------------------- | ----------------------------------------------------- | -------------------------------------- |
| Einstellungen und AirPlay-Schlüssel | `%APPDATA%\app.uwumirror.desktop\`      | `~/Library/Application Support/app.uwumirror.desktop` | `~/.local/share/app.uwumirror.desktop` |
| Die Webdaten des Fensters           | `%LOCALAPPDATA%\app.uwumirror.desktop\` | `~/Library/WebKit/app.uwumirror.desktop`              | `~/.local/share/app.uwumirror.desktop` |
| Das Programm                        | `%LOCALAPPDATA%\Programs\UwUMirror\`    | `/Applications/UwUMirror.app`                         | `/usr/bin/uwumirror-desktop`           |

Googles platform-tools liegen, wenn UwUMirror sie für dich heruntergeladen hat,
ebenfalls in UwUMirrors Datenordnern und gehen mit ihnen. Den
AirPlay-Schlüssel zu löschen schadet nicht: UwUMirror macht einen neuen, und
deine Geräte sehen einfach einen neuen Empfänger.

## Wenn etwas nicht klappt

- **Das Handy zeigt UwUMirror nicht an:** Fast immer das Netzwerk. Prüf die
  Firewall (oben), ob beide im selben Netz sind (nicht im Gastnetz) und ob das
  WLAN seine Geräte voneinander isoliert. Unter Windows muss das Netzwerk
  _Privat_ sein.
- **Bild, aber kein Ton (AirPlay):** Ob FFmpegs `libavcodec` fehlt, sagt
  UwUMirror in der App (unter Linux mit .rpm oder portablem Ordner siehe
  [Was UwUMirror vom System nutzt](#was-uwumirror-vom-system-nutzt)). Fehlt
  es nicht: in den Einstellungen das ausführliche Log einschalten, eine
  Minute mit Ton spiegeln und das Log an ein Issue hängen — darin steht, ob
  Ton ankam, dekodiert wurde und lauter als Stille war.
- **Kein Bild unter Linux, oder ein schwarzes:** Die H.264-Plugins von
  GStreamer fehlen — `gstreamer1.0-libav` / `gst-libav` installieren.
- **„adb not found“:** UwUMirror die platform-tools herunterladen lassen, oder
  `adb` / `android-tools` deines Systems installieren.
- **Android meldet „unauthorized“, oder das Koppeln klappt nicht:** das Handy
  entsperren und die Rückfrage darauf bestätigen; beim kabellosen Debugging mit
  einem frischen QR-Code neu koppeln.
- **„WebView2 couldn't be installed“** (Windows): Installiere die Evergreen
  WebView2 Runtime von
  [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/) und
  starte das Setup noch einmal.
- **Ein Virenscanner blockiert das Setup**: Das ist dasselbe fehlende Zertifikat
  wie bei SmartScreen. Der Quellcode jedes Releases liegt in diesem Repository,
  und die Prüfsummen zeigen dir, dass die Datei die veröffentlichte ist.
- **macOS sagt, die App sei beschädigt**: Das passiert, wenn die
  Quarantäne-Markierung an der installierten App hängen bleibt, was das Setup
  vermeidet. `xattr -dr com.apple.quarantine /Applications/UwUMirror.app`
  entfernt sie.
- **Windows auf ARM** hat ein eigenes Setup
  (`UwUMirror-windows-arm64-setup.exe`); das x64-Setup läuft dort auch,
  emuliert und langsamer.
- Etwas anderes? [Issue aufmachen](https://github.com/MinifyX/UwUMirror/issues),
  am besten mit Protokoll: **Einstellungen → Allgemein → Ausführliches
  Protokoll** an, noch mal versuchen, dann **Protokoll → Ordner öffnen** und
  `uwumirror.log` anhängen.
