# Architecture

How UwUMirror is put together: the crates, the protocols it speaks, how a
picture gets from a phone onto the screen, and how all of it is tested.

## The pieces

| Crate / app                | Job                                                                                                                       |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `crates/uwumirror-core`    | `StreamEvent` (the model every source speaks), sound output with cpal, AirPlay audio decoding through the system's FFmpeg |
| `crates/uwumirror-airplay` | The AirPlay receiver: Bonjour, RTSP, pairing, FairPlay, the mirroring stream, RTP audio, NTP                              |
| `crates/uwumirror-android` | Finding and running `adb`, pairing over wireless debugging, scrcpy's server and protocol                                  |
| `crates/uwumirror-cast`    | UwUCast between two UwUMirrors: mDNS, the receiver, and on Windows the sender (capture, Media Foundation, loopback sound) |
| `apps/desktop/src-tauri`   | The shell: starts the receiver and the Android side, the hub, commands for the page                                       |
| `apps/desktop/src`         | The page: React, the players, the start page, settings                                                                    |
| `apps/setup`               | The installer and uninstaller                                                                                             |

A source never knows the page. It produces `StreamEvent`s — `Started`,
`VideoSize`, `Video` (one H.264 access unit, Annex B), `VideoPaused`, `Audio`
(a status), `Ended` — into an `EventSink`. The desktop shell's hub
(`src-tauri/src/hub.rs`) turns them into JSON events for the interface and
binary messages on a Tauri channel for the video.

## From phone to screen

```
iPhone ──TCP 7000── RTSP ─► session.rs ── SETUP ─► mirror.rs ──► StreamEvent::Video
        ──TCP n──── 128-byte headers + AES-CTR H.264 ─┘                │
Android ── adb forward ── scrcpy video socket ──► scrcpy.rs ───────────┤
Windows PC ──TCP 7100── UwUCast messages ──► cast/receiver.rs ─────────┤
                                                                       ▼
                                                          hub.rs: cache since last key frame
                                                                       │ Channel (binary)
                                                                       ▼
                                          player.ts: WebCodecs → canvas, or fMP4 → Media Source
```

**Video is never decoded in Rust.** Every webview UwUMirror runs in can decode
H.264 with the system's own (usually hardware) decoder: WebView2 and
WKWebView through WebCodecs, WebKitGTK through GStreamer (WebCodecs or Media
Source). That keeps the app small, fast, and free of H.264 patent questions —
the same reason UwURDP doesn't ship an H.264 decoder either.

The player (`lib/player.ts`) prefers **WebCodecs**: `VideoDecoder` configured
from the SPS (`avc1.PPCCLL`) with an avcC built from SPS and PPS as its
`description`, frames length-prefixed (the form WebKit's WebCodecs takes too,
not only Chromium's), `optimizeForLatency`, every decoded frame drawn onto a
canvas at once. When WebCodecs is missing or fails ("not supported" at once,
three errors, or 120 frames without a picture), it switches to **Media Source**:
`lib/mp4.ts` wraps each access unit into a moof + mdat after an init segment
built from the SPS and PPS, and the player keeps the `<video>` within half a
second of the live edge. Settings → General → Video decoder forces either.

A player lives outside React for as long as its stream: its element moves into
the view that shows the stream, so switching tabs never restarts a decoder.

**The hub keeps each stream's frames since its last key frame** (at most 600
frames or 48 MB) and replays them to a page that subscribes, so a reload shows
a picture at once. iPhones send a new key frame only when the picture changes a
lot; a still screen can go minutes without one.

**Sound** is decoded in Rust and played with cpal (`core/audio.rs`): a short
buffer (60 ms to start), linear resampling to the device's rate, and a buffer
that is cut back to 100 ms whenever it grows past 300 ms. Live beats complete.

## AirPlay

UwUMirror speaks the AirPlay dialect the open-source receivers worked out —
UxPlay, RPiPlay, shairplay — as an Apple TV 3: features `0x5A7FFEE6`, legacy
pairing, FairPlay, H.264 only (the HEVC bit stays off, so senders never try it).

1. **Bonjour** (`advertise.rs`, with `mdns-sd`, no system service needed):
   `_airplay._tcp` as "Name" and `_raop._tcp` as "AABBCCDDEEFF@Name", both on
   the RTSP port — 7000, or the next free one (macOS's own receiver uses 7000).
   The TXT records carry the device id, features and the Ed25519 public key.
2. **`GET /info`** answers a binary plist: the TXT record asked for, or the
   whole description including the display the sender should aim for.
3. **Pairing** (`pairing.rs`): `/pair-setup` returns our Ed25519 key;
   `/pair-verify` trades X25519 keys, signs both with Ed25519, encrypted with
   AES-128-CTR under `SHA-512("Pair-Verify-AES-Key" ‖ secret)`. The receiver's
   key and its made-up MAC address are kept in `airplay-identity`.
4. **FairPlay** (`fairplay.rs`, `playfair/`): two `/fp-setup` rounds with fixed
   replies; SETUP's 72-byte `ekey` is unwrapped by playfair's
   `playfair_decrypt`. The key message's mode byte is checked before playfair
   sees it — playfair indexes its tables with it unchecked.
5. **SETUP** (`session.rs`): the first carries `ekey`/`eiv` (the session key is
   then `SHA-512(key ‖ pair-verify secret)[..16]`) and the sender's timing
   port; later ones ask for stream 110 (mirroring, a TCP port) and 96 (audio,
   two UDP ports).
6. **Mirroring** (`mirror.rs`): 128-byte headers, then payload. Type 1 is SPS
   and PPS in the clear (and the picture size); type 0 a frame, encrypted with
   AES-128-CTR under key and IV `SHA-512("AirPlayStreamKey"/"AirPlayStreamIV"
   - streamConnectionID ‖ session key)`, one key stream across all packets.
     NAL length prefixes become start codes.
7. **Audio** (`sound.rs`): RTP, the payload AES-128-CBC encrypted per packet
   (a tail shorter than a block stays clear), decoded by FFmpeg (AAC-ELD,
   AAC-LC, ALAC — `core/decode.rs` loads libavcodec and libavutil 58–63 at
   runtime and uses only the stable leading fields of three structs).
8. **Timing** (`timing.rs`): NTP-style requests to the sender every 3 s.

## Android

UwUMirror runs the `adb` it finds (`adb.rs`: a chosen path, the app's own
platform-tools, `PATH`, the Android SDK's usual places, Homebrew, `/usr/bin`)
or downloads Google's platform-tools on request (`platform_tools.rs`,
unpacked into a fresh folder and swapped in; paths that climb out are
skipped).

**Pairing** (`pairing.rs`) works like Android Studio: a QR code with
`WIFI:T:ADB;S:uwumirror-xxxxxx;P:password;;`; the phone scans it and announces
`_adb-tls-pairing._tcp` under that name; mDNS finds it and `adb pair` uses the
password. Then the phone's `_adb-tls-connect._tcp` port is found the same way
and connected.

**Mirroring** (`scrcpy.rs`) follows scrcpy 4.1's client: push the server to
`/data/local/tmp`, `adb forward tcp:0 localabstract:scrcpy_<id>`, start it with
`app_process`, connect the video socket (retried until the dummy byte comes),
then the audio socket; read the device name, the codec ids, then 12-byte
headers (session packets with the size, config packets with SPS/PPS, media
packets with key-frame flag and PTS). Sound is requested as raw PCM, 48 kHz
stereo. When the video socket ends — or the user stops — the server is killed
and the forward removed.

## UwUCast (computer to computer)

A Windows PC can't AirPlay, and Miracast needs Wi-Fi Direct and something
different on every system, so two UwUMirrors speak a small protocol of their
own (`crates/uwumirror-cast`). It carries exactly what the receiving side
already plays: H.264 in Annex B, as from an iPhone or scrcpy, and raw PCM, as
from Android. Nothing is decoded in Rust on either side.

**Finding each other** (`discovery.rs`): a receiver announces
`_uwumirror._tcp` on mDNS, IPv4 only, as `uwumirror-<random id>` — not by its
name, so two computers called the same don't share a record. The TXT record
says `name`, `version` (the app's), `proto` (UwUCast's version) and `id`; a
sender lists every receiver but the one with its own id, and won't send to
another protocol version.

**The wire** (`protocol.rs`), one TCP connection per stream (port 7100, or any
free one), big endian:

1. Hello, sender → receiver: magic `UwUCast\0`, protocol version (2 bytes),
   flags (bit 0: sound comes along), the sender's name and what it runs on
   ("Windows 11"), each a length byte and UTF-8 (at most 120 bytes).
2. Welcome, receiver → sender: the magic, its protocol version, a status
   (0 go ahead, 1 another version, 2 not receiving), its name.
3. Messages, each a type byte and a 4-byte length:

   | type   | payload                                                           | limit  |
   | ------ | ----------------------------------------------------------------- | ------ |
   | 1      | video: flags (bit 0 key frame), PTS in µs (8 bytes), Annex B data | 16 MiB |
   | 2      | picture size: width, height (4 bytes each, 1–16384)               | 8      |
   | 3      | sound: PTS in µs, then s16le PCM, 48 kHz, stereo, whole frames    | 1 s    |
   | 4      | end: the sender stops on purpose                                  | 0      |
   | `0x81` | receiver → sender: a key frame, please                            | 0      |

Lengths are checked before anything is allocated; an unknown type, a frame
without a start code or half a stereo frame ends the connection. A stream ends
cleanly with an end message or when the connection closes between two
messages; with a reason on garbage, or after 15 s of silence. The receiver
drops frames until the first key frame and asks for one (at most once a
second). Ending the stream at the receiver closes the connection, which the
sender reports as "ended by the receiver".

**Sending** (Windows, `screen/`): Windows.Graphics.Capture records the primary
screen with the pointer into a texture of our own (frames come only on
change; the latest is kept). D3D11's video processor converts BGRA to NV12 and
scales it into at most 1920 × 1080, proportions kept, on the card. Media
Foundation's H.264 encoder takes it: the graphics card's (asynchronous,
textures through a DXGI device manager, found for the capturing card by its
LUID) at 60 frames a second, or Windows' own (synchronous, frames copied back
to memory) at 30. Main profile, low latency, CBR at 10 Mbit/s, no B-frames, a
key frame every two seconds and on request, SPS and PPS in front of every key
frame. Colours are BT.709 where the encoder writes that into the stream (the
cards' do), BT.601 with Windows' own, which writes nothing. Sound is WASAPI's
loopback of the default output through cpal, resampled to 48 kHz stereo. The
capture threads never wait for the network: a full queue (a third of a
second) drops frames until the next key frame, which is asked for at once.

## Tests

Everything that can be tested without a phone is:

- **A whole AirPlay session** (`airplay/src/tests.rs`): a simulated iPhone
  runs `/info`, pair-setup, both pair-verify steps (checking our signature),
  both FairPlay rounds, both SETUPs, sends parameter sets and an encrypted IDR
  frame on the mirroring connection, sets up sound, sets the volume — and the
  test checks every event, the decrypted frame byte for byte, and that
  `end_stream` ends it.
- **The running app, fed by a simulated iPhone**: `mirror_a_file` (ignored by
  default) mirrors an H.264 file into a running UwUMirror; see the README.
  Used to check the picture on Linux with both decoders.
- **A whole Android mirror** (`android/tests/fake_phone.rs`): a shell script
  plays `adb` (logging every call), the test plays scrcpy's server on the
  forwarded port.
- **UwUCast against pretend senders** (`cast/tests/fake_sender.rs`): one that
  speaks the protocol byte by byte (frames before the first key frame dropped
  and a key frame asked for), the real sending half with made-up frames and
  an end from the receiver, another protocol version, garbage.
- **UwUCast for real, on Windows** (`cast/tests/send_screen.rs`, ignored by
  default): records this screen, encodes with the graphics card's encoder and
  with Windows' own, sends to a receiver in the same process and checks SPS,
  PPS and IDR in the first frame, the key frame interval and the end; and the
  loopback sound while a quiet tone plays. `UWUMIRROR_DUMP=folder` writes the
  streams out for `ffmpeg`. mDNS between two announcements is another ignored
  test (`discovery.rs`).
- **Real decoding**: AAC made by the `ffmpeg` command, decoded through the
  runtime-loaded libavcodec (`core/tests/decode.rs`).
- Units for RTSP parsing and limits, FairPlay rounds and mode checks, pairing,
  mirroring decryption across packets, AVCC to Annex B, audio decryption,
  resampling, the hub's cache, the platform-tools unpacking, UwUCast's
  messages and limits, NAL units and parameter sets, fitting the picture.

The checks CI runs (`.github/workflows/ci.yml`), and what to run before a push:

```bash
pnpm lint                     # Prettier and the i18n check
pnpm typecheck && pnpm -r build
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`installers.yml` builds and checks all installers for a tag; see
[release-notes/README.md](../release-notes/README.md).
