# Design

Clean, bright, soft — with a wink. Same design system as
[UwUSSH](https://github.com/MinifyX/UwUSSH-Client) and
[UwURDP](https://github.com/MinifyX/UwURDP-Client), same cat, one confident
bubblegum pink. The mirrored screen belongs to the phone: UwUMirror frames it
and otherwise keeps out of its way.

## Color

Tokens come from UwUMail unchanged, including the `--uwu-*` naming, and live in
`apps/desktop/src/styles/tokens.css`. Components never use raw hex values —
except the stage, below.

| Token              | Light     | Dark      | Use                                     |
| ------------------ | --------- | --------- | --------------------------------------- |
| `--uwu-canvas`     | `#f8f4f6` | `#141016` | App background                          |
| `--uwu-surface`    | `#ffffff` | `#1c171f` | Cards, title bar, tabs                  |
| `--uwu-elevated`   | `#fcf8fa` | `#241e28` | Hover rows, hints                       |
| `--uwu-ink`        | `#1c1420` | `#f8f2f6` | Primary text                            |
| `--uwu-muted`      | `#716672` | `#b3a8b3` | Secondary text                          |
| `--uwu-pink`       | `#ff4d8d` | `#ff7fac` | **Brand.** Selection, focus, active tab |
| `--uwu-pink-solid` | `#e11d74` | `#ff7fac` | Filled buttons with text                |
| `--uwu-online`     | `#17796a` | `#5cc7ac` | Receiver ready, stream live             |
| `--uwu-alarm`      | `#8e5510` | `#d8a25c` | Something needs attention               |

State uses semantic color, never pink: ready is mint, starting pulses pink
(the one exception, as in UwURDP's tabs), trouble is amber.

## The stage is always dark

Whatever the theme, a stream sits on a near-black stage (`#0c0a0d`): a light
frame around a phone screen glares, and letterboxing a portrait phone on a
wide window leaves a lot of frame. The picture is never tinted, filtered or
rounded. Waiting, pause and error messages on the stage use the dark palette
too.

## Type, shape, space

As in the family: **Manrope** (bundled) for the interface, **JetBrains Mono**
for addresses and sizes; radius 10 px for controls, 16 px for cards, pills for
badges; a 4 px grid; shadows only for dialogs, toasts and the full-screen bar.

## Layout

```
┌──────────────────────────────────────────────────────────┐
│ ◐ UwUMirror                                   ⚙  – □ ×  │  title bar
├──────────────────────────────────────────────────────────┤
│ [⌂ Start] [📱 Lorins iPhone ×] [🤖 Pixel 8 ×]           │  tabs
├──────────────────────────────────────────────────────────┤
│   Nyu + "Ready to mirror"                                │
│  ┌─ iPhone, iPad & Mac ── on ─┐ ┌─ Android ── Pair ──┐   │  start page
│  │ ● Ready as "UwUMirror (…)" │ │ 📶 Pixel 8  Mirror │   │
│  │ 1. 2. 3.                   │ │ How to get ready ▸ │   │
│  └────────────────────────────┘ └────────────────────┘   │
└──────────────────────────────────────────────────────────┘
```

- **The start page** says in one line whether UwUMirror is ready, then two
  cards: AirPlay (a switch, the name iPhones see, three steps, the FFmpeg hint
  when sound can't work) and Android (the phones adb knows, each with
  **Mirror**, and **Pair a phone**).
- **A tab per stream**, closing a tab ends the mirroring. The device's icon
  says iPhone, iPad, Mac, sound only or Android; a mint dot says live.
- **The stream view**: a quiet toolbar (name, source, size, sound, full
  screen, stop) over the stage. Waiting for the first picture shows Nyu
  connecting; a locked phone shows a pill "paused"; an undecodable stream
  explains what's missing.
- **Full screen** hides everything but the picture; a bar slides in from the
  top edge on hover, like mstsc's connection bar.
- **Pairing** is a dialog with three ways (QR code, pairing code, address),
  the QR code big and on white, the steps beside it, and Nyu cheering when it
  worked.

## Nyu, the mascot

Nyu is the same cat as in UwUMail, UwUSSH and UwURDP — this time she is a
**hand mirror**: an oval pink frame with a short handle, ears poking out
above it, a little mint gem on top, and a shine across the glass. The glass is
her face: UwU eyes, round `w` mouth, blush.

- **Sticker style**, unchanged: plum outlines `#4B1D3F`, pink body `#FF6FA6`,
  light glass `#FFB8D3`, a white die-cut edge. Fixed artwork, same in dark
  mode.
- **App icon** on UwUMail's pastel tile; **taskbar icon** Nyu alone, upright,
  a phone beside her with waves running into the glass, so it reads as
  mirroring. Sources in `brand/`, the React version in
  `apps/desktop/src/components/nyu/`; `node scripts/icons.mjs` regenerates the
  desktop icons.
- **Scenes** (320 × 220): welcome, waiting (a phone beaming at Nyu), connecting
  (the phone hops, the waves run), pair (Nyu holds up a phone with a code),
  done, load error (the waves broke off), puzzled, goodbye, sleepy.
- **Motion**: Nyu blinks, twitches her ears on hover; waves light up in turn.
  Settings → General → Animations (System / On / Off); reduced collapses
  everything to 1 ms.

## Tone of voice

Warm and a little playful; German is the source, "du", English follows the
system or the setting. Information first, short, kind, one kaomoji at most.
Anything about who may connect is plain.
