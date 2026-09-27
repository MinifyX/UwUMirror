/**
 * Shows one stream: H.264 in, pictures out.
 *
 * The system decodes, not us: WebCodecs where the webview has it (WebView2 on
 * Windows, WKWebView on macOS, newer WebKitGTK), hardware-accelerated and
 * with next to no latency, drawing each frame onto a canvas the moment it is
 * decoded. Where it is missing, the frames are wrapped as fragmented MP4 and
 * played through Media Source in a <video> element, kept close to the live
 * edge.
 *
 * A player lives as long as its stream, outside React: its element moves
 * into whichever view shows the stream, and a tab switch never costs a
 * decoder restart.
 */

import { codecString, parameterSets, splitNals, spsSize, type Nal } from './h264';
import { initSegment, mediaSegment, sample, TIMESCALE } from './mp4';
import { getSettings } from './settings';

export type PlayerBackend = 'webcodecs' | 'mediasource' | 'none';

export type PlayerInfo = {
  backend: PlayerBackend;
  width: number;
  height: number;
  frames: number;
  /** Set when nothing can decode the stream. */
  error: string | null;
};

type Backend = {
  push(key: boolean, pts: number, data: Uint8Array, nals: Nal[]): void;
  close(): void;
};

function equal(a: Uint8Array | null, b: Uint8Array): boolean {
  return !!a && a.length === b.length && a.every((v, i) => v === b[i]);
}

class WebCodecsBackend implements Backend {
  private decoder: VideoDecoder | null = null;
  private sps: Uint8Array | null = null;
  private waitingForKey = true;
  private failures = 0;
  private readonly context: CanvasRenderingContext2D | null;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly onFrame: (width: number, height: number) => void,
    private readonly onFail: (reason: string) => void,
  ) {
    this.context = canvas.getContext('2d', { alpha: false, desynchronized: true });
  }

  private configure(sps: Uint8Array) {
    this.decoder?.close();
    const decoder = new VideoDecoder({
      output: (frame) => {
        const { displayWidth: width, displayHeight: height } = frame;
        if (this.canvas.width !== width || this.canvas.height !== height) {
          this.canvas.width = width;
          this.canvas.height = height;
        }
        this.context?.drawImage(frame, 0, 0, width, height);
        frame.close();
        this.failures = 0;
        this.onFrame(width, height);
      },
      error: (error) => {
        // A decoder that failed is closed; the next key frame starts a new one.
        console.warn('video decoder', error);
        this.decoder = null;
        this.sps = null;
        this.waitingForKey = true;
        this.failures += 1;
        // "Not supported" won't get better with the next key frame (which an
        // iPhone may not send for a long time): hand over to Media Source now.
        if (error.name === 'NotSupportedError' || this.failures >= 3) this.onFail(String(error));
      },
    });
    // No `description`: the frames are Annex B, with SPS and PPS in band.
    decoder.configure({ codec: codecString(sps), optimizeForLatency: true });
    this.decoder = decoder;
    this.sps = sps;
  }

  push(key: boolean, pts: number, data: Uint8Array, nals: Nal[]) {
    if (key) {
      const sets = parameterSets(nals);
      if (sets && (!this.decoder || !equal(this.sps, sets.sps))) {
        try {
          this.configure(sets.sps);
        } catch (error) {
          this.onFail(String(error));
          return;
        }
      }
      if (this.decoder) this.waitingForKey = false;
    }
    if (this.waitingForKey || !this.decoder || this.decoder.state !== 'configured') return;
    this.decoder.decode(
      new EncodedVideoChunk({ type: key ? 'key' : 'delta', timestamp: pts, data }),
    );
  }

  close() {
    if (this.decoder && this.decoder.state !== 'closed') this.decoder.close();
    this.decoder = null;
  }
}

class MediaSourceBackend implements Backend {
  private source: MediaSource | null = null;
  private buffer: SourceBuffer | null = null;
  private queue: Uint8Array<ArrayBuffer>[] = [];
  private sps: Uint8Array | null = null;
  private sequence = 0;
  private time = 0;
  private lastPts: number | null = null;
  private url: string | null = null;

  constructor(
    private readonly video: HTMLVideoElement,
    private readonly sizeHint: () => { width: number; height: number },
    private readonly onFrame: (width: number, height: number) => void,
    private readonly onFail: (reason: string) => void,
  ) {
    video.addEventListener('resize', () => this.onFrame(video.videoWidth, video.videoHeight));
  }

  /** A new MediaSource for every new SPS: rotation changes the size, and not
   * every engine takes a second init segment with a different one. */
  private open(sps: Uint8Array, pps: Uint8Array) {
    this.teardown();
    const size = spsSize(sps) ?? this.sizeHint();
    const type = `video/mp4; codecs="${codecString(sps)}"`;
    if (!MediaSource.isTypeSupported(type)) {
      this.onFail(`unsupported: ${type}`);
      return;
    }
    const source = new MediaSource();
    this.source = source;
    this.sps = sps;
    this.queue = [initSegment(sps, pps, size.width, size.height)];
    this.sequence = 0;
    this.time = 0;
    this.lastPts = null;
    source.addEventListener('sourceopen', () => {
      if (this.source !== source) return;
      try {
        const buffer = source.addSourceBuffer(type);
        buffer.mode = 'segments';
        buffer.addEventListener('updateend', () => this.flush());
        this.buffer = buffer;
        this.flush();
      } catch (error) {
        this.onFail(String(error));
      }
    });
    this.url = URL.createObjectURL(source);
    this.video.src = this.url;
    void this.video.play().catch(() => undefined);
  }

  private flush() {
    const buffer = this.buffer;
    if (!buffer || buffer.updating || this.source?.readyState !== 'open') return;
    const next = this.queue.shift();
    if (next) {
      try {
        buffer.appendBuffer(next);
      } catch (error) {
        // QuotaExceeded: drop what was played and try again later.
        console.warn('media source', error);
        this.trim(true);
      }
      return;
    }
    this.chase();
  }

  /** Live is live: jump to the newest picture when playback falls behind. */
  private chase() {
    const { buffered } = this.video;
    if (buffered.length === 0) return;
    const end = buffered.end(buffered.length - 1);
    if (end - this.video.currentTime > 0.5) this.video.currentTime = Math.max(0, end - 0.05);
    if (this.video.paused) void this.video.play().catch(() => undefined);
    this.trim(false);
  }

  private trim(now: boolean) {
    const buffer = this.buffer;
    if (!buffer || buffer.updating) return;
    const start = this.video.currentTime - 10;
    if ((now || this.sequence % 300 === 0) && start > 0) {
      try {
        buffer.remove(0, start);
      } catch {
        // Removing is housekeeping; a failure only costs memory.
      }
    }
  }

  push(key: boolean, pts: number, _data: Uint8Array, nals: Nal[]) {
    if (key) {
      const sets = parameterSets(nals);
      if (sets && !equal(this.sps, sets.sps)) this.open(sets.sps, sets.pps);
    }
    if (!this.source) return;
    const duration =
      this.lastPts === null
        ? TIMESCALE / 60
        : Math.min(Math.max(((pts - this.lastPts) * TIMESCALE) / 1e6, 1), TIMESCALE / 5);
    this.lastPts = pts;
    // Parameter sets live in the init segment; delimiters mean nothing in MP4.
    const units = nals.filter((nal) => nal.type !== 7 && nal.type !== 8 && nal.type !== 9);
    if (units.length === 0) return;
    this.sequence += 1;
    this.queue.push(
      mediaSegment(
        this.sequence,
        Math.round(this.time),
        Math.round(duration),
        sample(units.map((u) => u.data)),
        key,
      ),
    );
    this.time += duration;
    // A page that can't keep up must not hoard minutes of video.
    if (this.queue.length > 240) this.queue.splice(1, this.queue.length - 120);
    this.flush();
  }

  private teardown() {
    this.buffer = null;
    if (this.source?.readyState === 'open') {
      try {
        this.source.endOfStream();
      } catch {
        // Already ending.
      }
    }
    this.source = null;
    if (this.url) URL.revokeObjectURL(this.url);
    this.url = null;
  }

  close() {
    this.teardown();
    this.video.removeAttribute('src');
    this.video.load();
  }
}

function webCodecsAvailable(): boolean {
  return (
    typeof window.VideoDecoder === 'function' && typeof window.EncodedVideoChunk === 'function'
  );
}

function mediaSourceAvailable(): boolean {
  return typeof window.MediaSource === 'function';
}

export class StreamPlayer {
  /** Goes into whichever view shows the stream. */
  readonly element: HTMLDivElement;
  private backend: Backend | null = null;
  private info: PlayerInfo = { backend: 'none', width: 0, height: 0, frames: 0, error: null };
  private readonly listeners = new Set<(info: PlayerInfo) => void>();
  private size = { width: 0, height: 0 };
  private triedMediaSource = false;

  constructor() {
    this.element = document.createElement('div');
    this.element.className = 'player';
    // Settings → General → Video decoder can force one; a new stream picks it up.
    const wanted = getSettings().decoder;
    if (webCodecsAvailable() && wanted !== 'mediasource') this.useWebCodecs();
    else this.useMediaSource();
  }

  private set(patch: Partial<PlayerInfo>) {
    this.info = { ...this.info, ...patch };
    for (const listener of this.listeners) listener(this.info);
  }

  private frame = (width: number, height: number) => {
    const first = this.info.frames === 0;
    this.info.frames += 1;
    if (first || width !== this.info.width || height !== this.info.height) {
      this.set({ width, height, error: null });
    }
  };

  private useWebCodecs() {
    const canvas = document.createElement('canvas');
    this.element.replaceChildren(canvas);
    this.backend = new WebCodecsBackend(canvas, this.frame, (reason) => {
      console.warn('WebCodecs gave up:', reason);
      this.useMediaSource();
    });
    this.set({ backend: 'webcodecs' });
  }

  private useMediaSource() {
    this.backend?.close();
    if (this.triedMediaSource || !mediaSourceAvailable()) {
      this.backend = null;
      this.set({ backend: 'none', error: 'no-decoder' });
      return;
    }
    this.triedMediaSource = true;
    const video = document.createElement('video');
    video.muted = true;
    video.autoplay = true;
    video.playsInline = true;
    video.disablePictureInPicture = true;
    this.element.replaceChildren(video);
    this.backend = new MediaSourceBackend(
      video,
      () => this.size,
      this.frame,
      (reason) => {
        console.warn('Media Source gave up:', reason);
        this.backend?.close();
        this.backend = null;
        this.set({ backend: 'none', error: 'no-decoder' });
      },
    );
    this.set({ backend: 'mediasource' });
  }

  /** The size the source announced, for engines that need it up front. */
  setSize(width: number, height: number) {
    this.size = { width, height };
  }

  push(key: boolean, pts: number, data: Uint8Array) {
    if (!this.backend) return;
    this.backend.push(key, pts, data, splitNals(data));
  }

  subscribe(listener: (info: PlayerInfo) => void): () => void {
    this.listeners.add(listener);
    listener(this.info);
    return () => this.listeners.delete(listener);
  }

  current(): PlayerInfo {
    return this.info;
  }

  close() {
    this.backend?.close();
    this.backend = null;
    this.listeners.clear();
    this.element.remove();
  }
}
