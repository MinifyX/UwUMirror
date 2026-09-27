/**
 * The commands Rust offers the page (see `src-tauri/src/lib.rs`), typed.
 */

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type StreamKind = 'airplay' | 'airplayaudio' | 'android';

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

export type AppInfo = { version: string; ffmpeg: number | null; scrcpy: string };

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

export const api = {
  computerName: () => invoke<string>('computer_name'),
  appInfo: () => invoke<AppInfo>('app_info'),
  ffmpegRecheck: () => invoke<number | null>('ffmpeg_recheck'),
  airplayApply: (settings: AirplaySettings) => invoke<AirplayStatus>('airplay_apply', { settings }),
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
  openLink: (url: string) => invoke<void>('open_link', { url }),
  logDetail: (on: boolean) => invoke<void>('log_detail', { on }),
  openLogFolder: () => invoke<void>('open_log_folder'),
};

/** Stream starts, changes and ends. */
export function onStreamMessage(handler: (message: StreamMessage) => void): Promise<UnlistenFn> {
  return listen<StreamMessage>('stream', (event) => handler(event.payload));
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

/** Video for every stream, from now on (and what each needs to start). */
export async function subscribeVideo(handler: (packet: VideoPacket) => void): Promise<void> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (message) => {
    const packet = parseVideo(message);
    if (packet) handler(packet);
  };
  await invoke('subscribe_video', { channel });
}

/** An error from a command, as text for a message. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
