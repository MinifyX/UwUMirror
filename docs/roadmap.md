# Roadmap

My wish list, roughly in order, without dates. It's a hobby project: things
get done when I feel like it.

## Next

- **Try it with real devices.** The first beta is tested against simulated
  senders only; the first real iPhones and Android phones will tell what's
  missing. Logs from those runs decide what comes first.
- **A PIN for AirPlay**, optional: the four digits Apple TVs show, so only who
  sees the screen can connect (AirPlay's pair-setup-pin with SRP-6a).
- **Controlling the Android phone** with mouse and keyboard: scrcpy's control
  socket is already there, UwUMirror just doesn't open it yet.
- **Automatic updates**, signed, as in UwURDP.

## Later

- AirPlay video casting (the AirPlay button in video apps, HLS), not just
  screen mirroring.
- HEVC for AirPlay where the webview can decode it (4K with less bandwidth).
- Recording a stream to a file.
- Sound for mirrored iPhones on Windows without installing FFmpeg — through a
  decoder the system has, if there is one for AAC-ELD.
- Remembering a window per device: size, position, full screen.

## Never

- A cloud relay, accounts, or telemetry.
- An Android app you have to install first.
