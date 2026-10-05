/**
 * The commands Rust offers the page (see `src-tauri/src/lib.rs`), typed.
 */

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type StreamKind = 'airplay' | 'airplayaudio' | 'android' | 'cast' | 'miracast';

export type AudioStatus = 'playing' | 'noDecoder' | 'noOutput' | 'unavailable' | 'off';

export type StreamState = {
  id: number;
  kind: StreamKind;
  name: string;
  model: string | null;
  address: string;
  width: number;
  height: number;
  audio: AudioStatus | null;
  paused: boolean;
};

export type StreamMessage =
  | { type: 'started'; stream: StreamState }
  | { type: 'updated'; stream: StreamState }
  | { type: 'ended'; id: number; name: string; kind: StreamKind; reason: string | null };

export type AppInfo = {
  version: string;
  ffmpeg: number | null;
  scrcpy: string;
  /** This build can send its screen (Windows). */
  castSend: boolean;
};

export type AirplaySettings = {
  enabled: boolean;
  name: string;
  width: number;
  height: number;
  fps: number;
  audio: boolean;
};

export type AirplayStatus = {
  running: boolean;
  port: number | null;
  name: string;
  error: string | null;
};

/** A device pairing with a PIN (`airplay/src/pin.rs`, `PairingEvent`). */
export type AirplayPairing =
  | { type: 'pinRequested'; pin: string; address: string }
  | { type: 'paired'; address: string; device: string }
  | {
      type: 'failed';
      address: string;
      reason: 'wrongPin' | 'expired' | 'locked' | 'broken';
    };

export type AdbStatus = {
  path: string | null;
  version: string | null;
  canDownload: boolean;
  own: boolean;
};

export type Device = {
  serial: string;
  state: string;
  model: string | null;
  wireless: boolean;
};

export type QrPairing = { name: string; password: string; svg: string };

export type MirrorOptions = { maxSize: number; bitRate: number; maxFps: number; audio: boolean };

export type VideoPacket = { id: number; key: boolean; pts: number; data: Uint8Array };

/** The Miracast receiver's state (`miracast/src/status.rs`). */
export type MiracastState =
  | 'unsupported'
  | 'off'
  | 'starting'
  | 'listening'
  | 'connected'
  | 'noWifiDirect'
  | 'wifiOff'
  | 'disabledByPolicy'
  | 'busy'
  | 'failed';

export type MiracastStatus = {
  state: MiracastState;
  /** The name senders list: Windows' own, the computer's. */
  name: string;
  /** A PIN to type on the sender, while it asks for one. */
  pin: string | null;
  error: string | null;
};

/** Windows' firewall for this program (`crates/uwumirror-firewall`). */
export type FirewallStatus = {
  /** False where there's nothing to set up (not Windows). */
  needed: boolean;
  /** Private networks let UwUMirror in. */
  private: boolean;
  /** Public networks — Miracast's Wi-Fi Direct — let its picture in. */
  miracast: boolean;
  /** UwUMirror's own two rules are there. */
  ours: boolean;
  ready: boolean;
  error: string | null;
};

/**
 * One decoded picture (Miracast's), NV12. `done` must be called once the
 * picture is no longer needed: it gives its memory back to Rust.
 */
export type RawFrame = {
  id: number;
  pts: number;
  width: number;
  height: number;
  data: Uint8Array;
  done: () => void;
};

/** Receiving from other computers' UwUMirror (UwUCast). */
export type CastSettings = { enabled: boolean; name: string };

export type CastStatus = { running: boolean; port: number | null; error: string | null };

/** Another computer's UwUMirror, found on the network. */
export type CastReceiver = { id: string; name: string; version: string; compatible: boolean };

export type SendEnd = { how: 'stopped' } | { how: 'byReceiver' } | { how: 'failed'; error: string };

/** Sending this screen; `ended` is set once, right after it ended. */
export type SendStatus = {
  state: 'idle' | 'connecting' | 'sending';
  id: string | null;
  receiver: string | null;
  width: number;
  height: number;
  fps: number;
  encoder: string | null;
  hardware: boolean;
  audio: boolean;
  ended: SendEnd | null;
};

export const api = {
  computerName: () => invoke<string>('computer_name'),
  appInfo: () => invoke<AppInfo>('app_info'),
  ffmpegRecheck: () => invoke<number | null>('ffmpeg_recheck'),
  airplayApply: (settings: AirplaySettings) => invoke<AirplayStatus>('airplay_apply', { settings }),
  airplayTrustedDevices: () => invoke<number>('airplay_trusted_devices'),
  airplayForgetDevices: () => invoke<void>('airplay_forget_devices'),
  streams: () => invoke<StreamState[]>('streams'),
  streamStop: (id: number) => invoke<boolean>('stream_stop', { id }),
  androidStatus: () => invoke<AdbStatus>('android_status'),
  androidDevices: () => invoke<Device[]>('android_devices'),
  androidQr: () => invoke<QrPairing>('android_qr'),
  androidPairQr: (qr: QrPairing) => invoke<void>('android_pair_qr', { qr }),
  androidPairCancel: () => invoke<void>('android_pair_cancel'),
  androidPairCode: (address: string, code: string) =>
    invoke<void>('android_pair_code', { address, code }),
  androidConnect: (address: string) => invoke<void>('android_connect', { address }),
  androidDisconnect: (serial: string) => invoke<void>('android_disconnect', { serial }),
  androidMirror: (serial: string, options: MirrorOptions) =>
    invoke<number>('android_mirror', { serial, options }),
  androidDownloadAdb: () => invoke<string>('android_download_adb'),
  androidChooseAdb: (path: string | null) => invoke<void>('android_choose_adb', { path }),
  miracastApply: (settings: { enabled: boolean; audio: boolean }) =>
    invoke<MiracastStatus>('miracast_apply', { settings }),
  firewallStatus: () => invoke<FirewallStatus>('firewall_status'),
  /** One administrator prompt; rejects with `declined` when it was declined. */
  firewallSetup: () => invoke<FirewallStatus>('firewall_setup'),
  castApply: (settings: CastSettings) => invoke<CastStatus>('cast_apply', { settings }),
  castReceivers: () => invoke<CastReceiver[]>('cast_receivers'),
  castSend: (id: string, name: string) => invoke<SendStatus>('cast_send', { id, name }),
  castSendStatus: () => invoke<SendStatus>('cast_send_status'),
  castSendStop: () => invoke<void>('cast_send_stop'),
  openLink: (url: string) => invoke<void>('open_link', { url }),
  logDetail: (on: boolean) => invoke<void>('log_detail', { on }),
  openLogFolder: () => invoke<void>('open_log_folder'),
};

/** Stream starts, changes and ends. */
export function onStreamMessage(handler: (message: StreamMessage) => void): Promise<UnlistenFn> {
  return listen<StreamMessage>('stream', (event) => handler(event.payload));
}

/** The Miracast receiver's changes. */
export function onMiracastStatus(handler: (status: MiracastStatus) => void): Promise<UnlistenFn> {
  return listen<MiracastStatus>('miracast', (event) => handler(event.payload));
}

/** A device asking for a PIN, and how its pairing ended. */
export function onAirplayPairing(handler: (event: AirplayPairing) => void): Promise<UnlistenFn> {
  return listen<AirplayPairing>('airplay-pairing', (event) => handler(event.payload));
}

/** Sending this screen: every change of state (see `src-tauri/src/cast.rs`). */
export function onCastSend(handler: (status: SendStatus) => void): Promise<UnlistenFn> {
  return listen<SendStatus>('cast-send', (event) => handler(event.payload));
}

/** Parses one binary video message (layout in `src-tauri/src/hub.rs`). */
export function parseVideo(buffer: ArrayBuffer): VideoPacket | null {
  if (buffer.byteLength < 17) return null;
  const view = new DataView(buffer);
  return {
    id: Number(view.getBigUint64(0, true)),
    key: (view.getUint8(8) & 1) === 1,
    pts: Number(view.getBigUint64(9, true)),
    data: new Uint8Array(buffer, 17),
  };
}

/** A decoded picture that came through the channel (flags 2, see `hub.rs`). */
export function parseFrame(buffer: ArrayBuffer): RawFrame | null {
  if (buffer.byteLength < 25) return null;
  const view = new DataView(buffer);
  if ((view.getUint8(8) & 2) === 0) return null;
  const width = view.getUint32(17, true);
  const height = view.getUint32(21, true);
  if (buffer.byteLength < 25 + (width * height * 3) / 2) return null;
  return {
    id: Number(view.getBigUint64(0, true)),
    pts: Number(view.getBigUint64(9, true)),
    width,
    height,
    data: new Uint8Array(buffer, 25, (width * height * 3) / 2),
    done: () => void invoke('frame_done').catch(() => undefined),
  };
}

/** Video and decoded pictures for every stream, from now on (and what each
 * needs to start). */
export async function subscribeVideo(
  handler: (packet: VideoPacket) => void,
  onFrame: (frame: RawFrame) => void,
): Promise<void> {
  subscribeSharedFrames(onFrame);
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (message) => {
    const frame = parseFrame(message);
    if (frame) {
      onFrame(frame);
      return;
    }
    const packet = parseVideo(message);
    if (packet) handler(packet);
  };
  await invoke('subscribe_video', { channel });
}

/** Bytes in front of a picture in a shared buffer (`SLOT_HEADER` in
 * `src-tauri/src/frames.rs`); byte 0 says who has the buffer. */
const FRAME_HEADER = 64;

type SharedBufferEvent = Event & {
  additionalData: unknown;
  getBuffer(): ArrayBuffer;
};

type WebView2 = {
  addEventListener(
    type: 'sharedbufferreceived',
    listener: (event: SharedBufferEvent) => void,
  ): void;
  releaseBuffer(buffer: ArrayBuffer): void;
};

let sharedFrames = false;

/**
 * Pictures WebView2 shares with the page (Windows; see `frames.rs`): memory
 * Rust wrote the picture into, posted with its size and time. Done with it,
 * the page sets byte 0 back to 0 and lets go of the mapping.
 */
function subscribeSharedFrames(onFrame: (frame: RawFrame) => void) {
  const webview = (window as { chrome?: { webview?: WebView2 } }).chrome?.webview;
  if (!webview || sharedFrames) return;
  sharedFrames = true;
  webview.addEventListener('sharedbufferreceived', (event) => {
    const meta = event.additionalData as {
      uwumirror?: string;
      id: number;
      width: number;
      height: number;
      pts: number;
    } | null;
    if (meta?.uwumirror !== 'frame') return;
    const buffer = event.getBuffer();
    const size = (meta.width * meta.height * 3) / 2;
    let released = false;
    const done = () => {
      if (released) return;
      released = true;
      new Uint8Array(buffer, 0, 1)[0] = 0;
      webview.releaseBuffer(buffer);
    };
    if (buffer.byteLength < FRAME_HEADER + size) {
      done();
      return;
    }
    onFrame({
      id: meta.id,
      pts: meta.pts,
      width: meta.width,
      height: meta.height,
      data: new Uint8Array(buffer, FRAME_HEADER, size),
      done,
    });
  });
}

/** An error from a command, as text for a message. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
