<p align="center">
  <img src="brand/uwumirror-app-icon.svg" width="112" alt="UwUMirror logo" />
</p>

<h1 align="center">UwUMirror</h1>

<p align="center">
  Your phone's screen on your computer, in your own network. (◕‿◕✿)<br/>
  AirPlay · Android · Windows, macOS and Linux, first beta
</p>

<p align="center">
  <a href="https://github.com/MinifyX/UwUMirror/releases"><b>Download for Windows, macOS and Linux</b></a>
  ·
  <a href="docs/install.md"><b>How to install</b></a>
  ·
  <a href="docs/install.md#uwumirror-installieren">Anleitung auf Deutsch</a>
</p>

---

## Why this exists

I wanted to put my phone on the big screen now and then — a video, some
photos, an app to show someone — and every way to do that annoyed me.
AirPlay receivers for Windows cost money and want an account. The free ones
show ads, or send your screen through somebody's cloud. Android has no
standard way at all: every vendor has its own casting that works with its own
TVs. scrcpy is brilliant, but a terminal program. So I built a small receiver
of my own, in the same family as my other UwU apps.

- **Just for fun.** No company, no team, no schedule, no promises. I work on it
  when I have time and feel like it, so don't expect steady development, and
  don't be surprised by long breaks.
- **Written with AI.** Almost all of the code is written with Claude, because
  I'm honestly not a great programmer. Not your thing? No hard feelings, just
  pick something else.
- **Use it, fork it, do what you want with it.** The license only asks one
  thing: if you pass on a changed version, or run one for others over a
  network, its source stays open too.
- **No support.** Issues and pull requests are okay, but I might answer late or
  not at all, and I mostly build what I need myself.

## What it is

UwUMirror turns your computer into a screen your phone can mirror to. It is
simple on purpose: open it, and it is ready.

- **iPhone, iPad and Mac over AirPlay.** UwUMirror shows up under **Screen
  Mirroring** in the Control Center, like an Apple TV. Pick it, and your
  screen is here — with sound. AirPlay audio from Music and Podcasts works
  too.
- **Android over wireless debugging.** Pair a phone once by scanning a QR code
  (or with the six-digit code, or over USB), and from then on it shows up in
  UwUMirror by itself; one click mirrors it, with sound on Android 11 and
  newer. It uses the server of [scrcpy](https://github.com/Genymobile/scrcpy),
  the best there is for this.
- **Several at once.** Every stream gets a tab. Full screen with F11 or a
  double click, Escape to leave it. Handy with a TV or projector on the
  computer.
- **Your network, nothing else.** No cloud, no account, no telemetry. The
  picture goes straight from the phone to this computer, and nowhere else.
- **Playful.** Nyu, the cat, is a hand mirror this time.

> **Status: first beta.** 0.1.0-beta.1 is built by CI for Windows (x64 and
> ARM), macOS (Apple silicon and Intel) and Linux (x64 and ARM).
>
> **An honest warning:** UwUMirror has not met a real
> iPhone or Android phone yet. Everything between the network and the picture
> is tested — against a simulated iPhone that pairs, does FairPlay and sends
> encrypted H.264, and against a simulated Android phone behind a pretend
> `adb` — and the app shows that picture on Linux, with both of its decoders.
> But AirPlay is a reverse-engineered protocol, and the first real iPhone may
> well find something the simulation didn't. If it doesn't work for you, an
> issue with the log helps.
>
> **What works.**
>
> - AirPlay screen mirroring (H.264, up to 4K at 60 frames a second, as the
>   sender manages), with legacy pairing and FairPlay, the dialect the
>   open-source receivers before it speak ([UxPlay](https://github.com/FDH2/UxPlay),
>   RPiPlay, shairplay). Sound through FFmpeg's libavcodec from the system.
> - AirPlay audio (ALAC, AAC) from Music, Podcasts and friends.
> - Android: pairing by QR code or pairing code, connecting by address, USB,
>   mirroring with scrcpy 4.1's server, raw PCM sound from Android 11 on.
>   Google's platform-tools (adb) download on request.
> - Video decoded by the system: WebCodecs where the webview has it, Media
>   Source otherwise, switchable in the settings.
>
> **What doesn't, yet.** Controlling the phone with mouse and keyboard,
> AirPlay video casting (the "AirPlay" button in video apps, not screen
> mirroring), HEVC, a PIN for AirPlay, recording, automatic updates. The
> [roadmap](docs/roadmap.md) has the order.

## Install

Windows 10 or 11 (x64 and ARM), macOS 11 or newer (Apple silicon and Intel),
Linux (x86_64 and arm64).

1. Open the [releases](https://github.com/MinifyX/UwUMirror/releases) and
   download the file for your system from the newest one:
   `UwUMirror-windows-x64-setup.exe` (`UwUMirror-windows-arm64-setup.exe` on
   ARM), `UwUMirror-macos-universal.dmg`, or on Linux `UwUMirror-linux-x64.deb`
   / `.rpm` (`…-arm64…` on ARM), the release's `PKGBUILD` on Arch (`makepkg -si`), or the
   `…-portable.tar.gz` to just unpack and run.
2. Run it. Neither Windows nor macOS knows the setup, because it isn't signed
   with a paid certificate: on Windows **More info → Run anyway**, on macOS
   **System Settings → Privacy & Security → Open Anyway**.
3. Click **Install**. No admin prompt: it installs for your user only.
4. The first start asks whether UwUMirror may accept connections: allow it for
   **private networks**, or iPhones won't find it.

The [install guide](docs/install.md) has the details: what UwUMirror uses from
your system (FFmpeg for AirPlay sound, adb for Android, GStreamer on Linux),
the firewall, uninstalling, and what to do when something goes wrong.
[Auf Deutsch](docs/install.md#uwumirror-installieren).

## How to mirror

**iPhone, iPad, Mac:** same Wi-Fi as the computer → Control Center →
**Screen Mirroring** → pick "UwUMirror (your computer)". On the Mac it is the
Control Center's Screen Mirroring too.

**Android:** once — Settings → About phone → tap **Build number** seven times;
then Developer options → **Wireless debugging** on → **Pair device with QR
code**, and scan the code from **Pair a phone** in UwUMirror. After that, the
phone is in the list whenever wireless debugging is on; click **Mirror**.

## Project layout

| Path                       | What lives there                                                         |
| -------------------------- | ------------------------------------------------------------------------ |
| `apps/desktop`             | The Tauri 2 app (React UI + Rust shell)                                  |
| `apps/setup`               | The installer and uninstaller, for all three systems                     |
| `crates/uwumirror-core`    | The stream model, sound output (cpal), AirPlay audio through FFmpeg      |
| `crates/uwumirror-airplay` | The AirPlay receiver: Bonjour, RTSP, pairing, FairPlay, mirroring, sound |
| `crates/uwumirror-android` | adb, wireless-debugging pairing, scrcpy's protocol                       |
| `brand/`                   | Nyu: the UwUMirror icon, symbol, mono symbol, taskbar icons              |
| `docs/`                    | Install guide, architecture, design, roadmap                             |
| `release-notes/`           | What's new, per version                                                  |
| `scripts/`                 | Building the setup, icons, the scrcpy server, the AUR package            |
| `packaging/aur`            | The Arch package                                                         |

## Development

Requirements:

- Node.js 22 or newer and pnpm 10 (`corepack enable`)
- Rust stable (via [rustup](https://rustup.rs)) and a C compiler (for
  playfair, see `crates/uwumirror-airplay/playfair`)
- Platform prerequisites for Tauri: see
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
  (Windows: Visual Studio C++ Build Tools and WebView2; Linux:
  `libwebkit2gtk-4.1-dev` and friends, plus `libasound2-dev` for sound)

```bash
pnpm install
node scripts/fetch-scrcpy-server.mjs   # scrcpy's server, checked by hash
pnpm tauri dev
```

No iPhone at hand? A test plays one against the running app and mirrors any
H.264 file to it (Annex B with access unit delimiters, e.g. from
`ffmpeg … -c:v libx264 -bf 0 -x264-params aud=1 -f h264 clip.h264`):

```bash
UWUMIRROR_PORT=7000 UWUMIRROR_VIDEO=clip.h264 \
  cargo test -p uwumirror-airplay -- --ignored --nocapture mirror_a_file
```

Checks:

```bash
pnpm typecheck && pnpm lint
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

The installer, with the app packed inside:

```bash
pnpm build:setup                  # target/installers/, for the system it runs on
```

A release is a tag: CI builds all installers, checks them, and drafts the
release with the notes from `release-notes/`.
[release-notes/README.md](release-notes/README.md) has the steps.

## Documentation

- [Install guide](docs/install.md) — installing, what UwUMirror needs, uninstalling, in English and German
- [Konzept](KONZEPT.md) — the concept, in German
- [Architecture](docs/architecture.md) — how the pieces fit together, and the protocols
- [Design](docs/design.md) — colors, type, Nyu, tone of voice
- [Roadmap](docs/roadmap.md) — my wish list, without dates

## License

UwUMirror is free software under the [GNU AGPL v3.0](LICENSE): use it, change
it, fork it, share it. If you pass on a changed version, or let others use one
over a network, its source has to stay open too.

It stands on the shoulders of others:

- **playfair**, the FairPlay decryption AirPlay mirroring needs, taken from
  [UxPlay](https://github.com/FDH2/UxPlay) (GNU GPL v3), which the AGPL may be
  combined with. The AirPlay receiver follows UxPlay's, RPiPlay's and
  shairplay's work on the protocol.
- **scrcpy's server** ([Genymobile/scrcpy](https://github.com/Genymobile/scrcpy),
  Apache-2.0) is shipped with the app and runs on the Android phone.
- **FFmpeg** (LGPL) and **adb** are not shipped: UwUMirror uses them from the
  system, and downloads Google's platform-tools from Google only when asked.

AirPlay, iPhone, iPad and Mac are trademarks of Apple Inc.; Android is a
trademark of Google LLC. UwUMirror has nothing to do with either.
