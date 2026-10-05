# Design

Clean, bright, soft — with a wink. Same design system as
[UwUSSH](https://github.com/MinifyX/UwUSSH-Client) and
[UwURDP](https://github.com/MinifyX/UwURDP-Client), same cat, one confident
bubblegum pink. The mirrored screen belongs to the phone: UwUMirror frames it
and otherwise keeps out of its way.

## Color

Tokens come from UwUMail, including the `--uwu-*` naming, and live in
`apps/desktop/src/styles/tokens.css`. Components never use raw hex values —
except the stage, below. Light by default, following the system; Settings →
General → Colour scheme (System / Light / Dark) picks one.

| Token              | Light     | Dark      | Use                                   |
| ------------------ | --------- | --------- | ------------------------------------- |
| `--uwu-canvas`     | `#f8f4f6` | `#141016` | App background                        |
| `--uwu-surface`    | `#ffffff` | `#1c171f` | Cards, dialogs, stream toolbar        |
| `--uwu-elevated`   | `#fcf8fa` | `#241e28` | Hover rows, hints                     |
| `--uwu-ink`        | `#1c1420` | `#f8f2f6` | Primary text                          |
| `--uwu-muted`      | `#716672` | `#b3a8b3` | Secondary text                        |
| `--uwu-pink`       | `#ff4d8d` | `#ff7fac` | **Brand.** Selection, focus, wordmark |
| `--uwu-pink-solid` | `#e11d74` | `#ff7fac` | Filled buttons with text              |
| `--uwu-online`     | `#17796a` | `#5cc7ac` | Receiver ready, stream live           |
| `--uwu-alarm`      | `#8e5510` | `#d8a25c` | Something needs attention             |

State uses semantic color, never pink: ready is mint, starting pulses pink
(the one exception, as in UwURDP's tabs), trouble is amber.

## The stage is always dark

Whatever the theme, a stream sits on a near-black stage (`#0c0a0d`): a light
frame around a phone screen glares, and letterboxing a portrait phone on a
wide window leaves a lot of frame. The picture is never tinted, filtered or
rounded. Waiting, pause and error messages on the stage use the dark palette
too.

## Type, shape, space

As in UwUMail: **UwU Sans** (Atkinson Hyperlegible Next with Nyu, bundled,
SIL OFL 1.1, `apps/desktop/src/assets/fonts/`, byte-identical to UwUMail's)
for the interface, **JetBrains Mono** for addresses and sizes; radius 10 px
for controls, 16 px for cards, 22 px for dialogs, pills for states and badges;
a 4 px grid; shadows only for dialogs, toasts and the full-screen bar.

## Layout

No sidebar: a slim bar on top, the page under it.

```
┌──────────────────────────────────────────────────────────┐
│ ◐ UwUMirror                      [⇪ Send]   ⚙   – □ ×   │  title bar
├──────────────────────────────────────────────────────────┤
│   Nyu   Ready to mirror                                  │
│         Devices find this computer as (UwUMirror (PC))   │  start page
│  MIRRORING NOW   📱 Lorins iPhone        [Show] [Stop]   │
│  HOW TO MIRROR HERE                                      │
│  ┌ iPhone, iPad & Mac  AirPlay ● Ready ┐ ┌ Android … ┐   │
│  │ Control Center → Screen Mirroring   │ │ Win+K …   │   │
│  └─────────────────────────────────────┘ └───────────┘   │
└──────────────────────────────────────────────────────────┘
```

- **The start page stays clean**: no switches, no forms. Nyu, "Ready to
  mirror" and the name devices look for; whatever mirrors right now with
  **Show** and **Stop**; then one calm card per way in (AirPlay; Miracast on
  Windows; wireless debugging; other computers), each with one line of
  how-to and its state as a pill (Ready / Starting / Off / Problem). Off or a
  problem links to the right settings section. Phones paired for wireless
  debugging that are reachable get a **Mirror** button there.
- **Settings** is UwUMail's large dialog: sections with icons on the left
  (on top in a narrow window) — General, AirPlay, Miracast (Windows),
  Android (debugging), Other computers, About — and rows beside them. Every
  switch lives here, including pairing and the paired phones.
- **Send** (Windows) sits in the title bar as a pill and opens a dialog with
  the receivers; while sending, the pill turns pink and names the receiver.
- **The stream view**: a quiet toolbar (Start, name, source, size, sound, full
  screen, stop) in the theme's colours over the dark stage. Waiting for the
  first picture shows Nyu connecting; a locked phone shows a pill "paused"; an
  undecodable stream explains what's missing. Ctrl+0 goes back to the start
  page, Ctrl+1 to the stream.
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
