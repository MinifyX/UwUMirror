/**
 * The settings, kept in the page's own storage.
 *
 * Only preferences live here. The AirPlay receiver and the Android side run
 * in Rust; the page hands them what they need (see `App.tsx`) on start and
 * whenever a setting that concerns them changes.
 */

import { useSyncExternalStore } from 'react';
import { language } from './i18n';

export type ThemeSetting = 'system' | 'light' | 'dark';
/** German or English; "system" follows the language the system prefers. */
export type LanguageSetting = 'system' | 'de' | 'en';
/** Animations: follow the system's reduced-motion setting, or override it. */
export type MotionSetting = 'system' | 'on' | 'off';
/** The picture size iPhones are asked to send. */
export type AirplayResolution = '720p' | '1080p' | '1440p' | '2160p';
/** Which way the page decodes video; "auto" prefers WebCodecs. */
export type DecoderSetting = 'auto' | 'webcodecs' | 'mediasource';

export const RESOLUTIONS: Record<AirplayResolution, [number, number]> = {
  '720p': [1280, 720],
  '1080p': [1920, 1080],
  '1440p': [2560, 1440],
  '2160p': [3840, 2160],
};

export type Settings = {
  language: LanguageSetting;
  theme: ThemeSetting;
  motion: MotionSetting;
  /** Show a new stream's tab as soon as it starts. */
  showNewStreams: boolean;
  /** …and go full screen with it. */
  fullscreenNewStreams: boolean;
  /** For systems where one of the two decoders misbehaves. */
  decoder: DecoderSetting;

  airplayEnabled: boolean;
  /** What iPhones show in their AirPlay list; empty means "UwUMirror (computer)". */
  receiverName: string;
  airplayResolution: AirplayResolution;
  airplayFps: 30 | 60;
  airplayAudio: boolean;

  /** The longer side of an Android picture; 0 keeps the phone's own size. */
  androidMaxSize: 0 | 1280 | 1920 | 2560;
  /** Video bit rate in Mbit/s. */
  androidBitRate: 4 | 8 | 16 | 24;
  androidFps: 30 | 60;
  androidAudio: boolean;
  /** An adb chosen by hand; empty means "find one". */
  adbPath: string;
};

export const DEFAULT_SETTINGS: Settings = {
  language: 'system',
  theme: 'dark',
  motion: 'system',
  showNewStreams: true,
  fullscreenNewStreams: false,
  decoder: 'auto',
  airplayEnabled: true,
  receiverName: '',
  airplayResolution: '1080p',
  airplayFps: 60,
  airplayAudio: true,
  androidMaxSize: 1920,
  androidBitRate: 8,
  androidFps: 60,
  androidAudio: true,
  adbPath: '',
};

const KEY = 'uwumirror.settings';

/** Stored values are checked one by one; anything unexpected falls back to its default. */
export function sanitize(raw: unknown): Settings {
  const input = typeof raw === 'object' && raw !== null ? (raw as Record<string, unknown>) : {};
  const oneOf = <T>(value: unknown, allowed: readonly T[], fallback: T): T =>
    allowed.includes(value as T) ? (value as T) : fallback;
  const bool = (value: unknown, fallback: boolean) =>
    typeof value === 'boolean' ? value : fallback;
  const text = (value: unknown, max: number) =>
    typeof value === 'string' ? value.slice(0, max) : '';
  const d = DEFAULT_SETTINGS;
  return {
    language: oneOf(input.language, ['system', 'de', 'en'] as const, d.language),
    theme: oneOf(input.theme, ['system', 'light', 'dark'] as const, d.theme),
    motion: oneOf(input.motion, ['system', 'on', 'off'] as const, d.motion),
    showNewStreams: bool(input.showNewStreams, d.showNewStreams),
    fullscreenNewStreams: bool(input.fullscreenNewStreams, d.fullscreenNewStreams),
    decoder: oneOf(input.decoder, ['auto', 'webcodecs', 'mediasource'] as const, d.decoder),
    airplayEnabled: bool(input.airplayEnabled, d.airplayEnabled),
    receiverName: text(input.receiverName, 60),
    airplayResolution: oneOf(
      input.airplayResolution,
      ['720p', '1080p', '1440p', '2160p'] as const,
      d.airplayResolution,
    ),
    airplayFps: oneOf(input.airplayFps, [30, 60] as const, d.airplayFps),
    airplayAudio: bool(input.airplayAudio, d.airplayAudio),
    androidMaxSize: oneOf(input.androidMaxSize, [0, 1280, 1920, 2560] as const, d.androidMaxSize),
    androidBitRate: oneOf(input.androidBitRate, [4, 8, 16, 24] as const, d.androidBitRate),
    androidFps: oneOf(input.androidFps, [30, 60] as const, d.androidFps),
    androidAudio: bool(input.androidAudio, d.androidAudio),
    adbPath: text(input.adbPath, 1024),
  };
}

function load(): Settings {
  try {
    const raw = window.localStorage.getItem(KEY);
    return sanitize(raw ? JSON.parse(raw) : {});
  } catch {
    return DEFAULT_SETTINGS;
  }
}

let current = load();
const listeners = new Set<() => void>();

export function getSettings(): Settings {
  return current;
}

export function updateSettings(patch: Partial<Settings>) {
  current = sanitize({ ...current, ...patch });
  try {
    window.localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Private storage can be unavailable; the change still holds for this run.
  }
  for (const listener of listeners) listener();
}

export function subscribeSettings(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useSettings(): Settings {
  return useSyncExternalStore(subscribeSettings, getSettings);
}

/** The name iPhones see: the user's, or "UwUMirror (computer name)". */
export function receiverName(settings: Settings, computer: string): string {
  const own = settings.receiverName.trim();
  if (own) return own;
  return computer ? `UwUMirror (${computer})` : 'UwUMirror';
}

const darkQuery = () => window.matchMedia('(prefers-color-scheme: dark)');
const reducedQuery = () => window.matchMedia('(prefers-reduced-motion: reduce)');

/** Whether animations should play right now, by setting and system. */
export function motionAllowed(): boolean {
  const { motion } = current;
  return motion === 'on' || (motion === 'system' && !reducedQuery().matches);
}

/** Puts theme and motion on <html>, now and whenever the setting or the system changes. */
export function applyAppearance() {
  const apply = () => {
    const { theme } = current;
    const dark = theme === 'dark' || (theme === 'system' && darkQuery().matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    document.documentElement.lang = language(current);
    if (motionAllowed()) delete document.documentElement.dataset.motion;
    else document.documentElement.dataset.motion = 'reduced';
  };
  apply();
  subscribeSettings(apply);
  darkQuery().addEventListener('change', apply);
  reducedQuery().addEventListener('change', apply);
}
