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
 * Miracast's pictures come decoded already (Windows decodes them): those go
 * onto the same canvas as WebCodecs `VideoFrame`s made straight from their
 * bytes.
 *
 * A player lives as long as its stream, outside React: its element moves
 * into whichever view shows the stream, and a tab switch never costs a
 * decoder restart.
 */

import { api, type RawFrame } from './api';
import { codecString, parameterSets, splitNals, spsSize, type Nal } from './h264';
import { avcConfig, initSegment, mediaSegment, sample, TIMESCALE } from './mp4';
import { getSettings } from './settings';

export type PlayerBackend = 'webcodecs' | 'mediasource' | 'frames' | 'none';

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

/** Chunks a decoder may swallow without a single picture before it counts
 * as broken: some engines neither decode nor complain. Two seconds at 60 fps,
 * far more than any decoder's pipeline holds. */
const SILENT_CHUNKS = 120;

/** Parameter sets and delimiters stay out of the samples: the sets travel in
 * the description, delimiters mean nothing outside Annex B. */
function frameUnits(nals: Nal[]): Uint8Array[] {
  return nals
    .filter((nal) => nal.type !== 7 && nal.type !== 8 && nal.type !== 9)
    .map((nal) => nal.data);
}

/** How often the page's latency goes into the log. */
const LATENCY_EVERY_MS = 5000;
/** Further from now than this, a timestamp isn't the sender's clock. */
const PLAUSIBLE_MS = 5000;

function summary(values: number[]): string {
  const sorted = [...values].sort((a, b) => a - b);
  const at = (q: number) => (sorted[Math.round((sorted.length - 1) * q)] ?? 0).toFixed(1);
  return `median ${at(0.5)} ms, p95 ${at(0.95)} ms, max ${at(1)} ms over ${sorted.length} frames`;
}

/**
 * How late pictures are on the page, into the app's log every few seconds
 * (with the detailed log on): from the page receiving a frame to drawing
 * it, which is the decoder; and, for UwUCast, whose senders stamp frames
 * with their wall clock, from capture to drawing — everything, the network
 * and the hub included. Other sources' timestamps count from their own
 * start and are left out of the second.
 */
class LatencyMeter {
  /** When the page got each frame not yet drawn, by timestamp. */
  private readonly arrived = new Map<number, number>();
  private toDrawn: number[] = [];
  private fromCapture: number[] = [];
  private since = performance.now();

  received(pts: number) {
    // A decoder that drops frames mustn't grow this for ever.
    if (this.arrived.size > 600) this.arrived.clear();
    this.arrived.set(pts, performance.now());
  }

  drawn(pts: number) {
    const now = performance.now();
    const at = this.arrived.get(pts);
    if (at !== undefined) {
      this.arrived.delete(pts);
      this.toDrawn.push(now - at);
    }
    const age = performance.timeOrigin + now - pts / 1000;
    if (Math.abs(age) < PLAUSIBLE_MS) this.fromCapture.push(age);
    if (now - this.since >= LATENCY_EVERY_MS) this.report(now);
  }

  private report(now: number) {
    const parts: string[] = [];
    if (this.toDrawn.length > 0) parts.push(`received → drawn: ${summary(this.toDrawn)}`);
    if (this.fromCapture.length > 0) parts.push(`capture → drawn: ${summary(this.fromCapture)}`);
    this.toDrawn = [];
    this.fromCapture = [];
    this.since = now;
    if (parts.length === 0) return;
    const report = parts.join('; ');
    console.debug('video latency', report);
    void api.videoLatency(report).catch(() => undefined);
  }
}

class WebCodecsBackend implements Backend {
  private decoder: VideoDecoder | null = null;
  private sps: Uint8Array | null = null;
  private pps: Uint8Array | null = null;
  private waitingForKey = true;
  private failures = 0;
  private decodedAny = false;
  private sent = 0;
  private readonly context: CanvasRenderingContext2D | null;
  private readonly latency = new LatencyMeter();

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly onFrame: (width: number, height: number) => void,
    private readonly onFail: (reason: string) => void,
  ) {
    this.context = canvas.getContext('2d', { alpha: false, desynchronized: true });
  }

  private configure(sps: Uint8Array, pps: Uint8Array) {
    this.decoder?.close();
    const decoder = new VideoDecoder({
      // Drawn the moment it is decoded, not at the next animation frame: a
      // desynchronized canvas shows it at the next refresh, and a newer
      // frame decoded before then simply draws over it.
      output: (frame) => {
        const { displayWidth: width, displayHeight: height } = frame;
        if (this.canvas.width !== width || this.canvas.height !== height) {
          this.canvas.width = width;
          this.canvas.height = height;
        }
        this.context?.drawImage(frame, 0, 0, width, height);
        this.latency.drawn(frame.timestamp);
        frame.close();
        this.failures = 0;
        this.decodedAny = true;
        this.onFrame(width, height);
      },
      error: (error) => {
        // A decoder that failed is closed; the next key frame starts a new one.
        console.warn('video decoder', error);
        this.decoder = null;
        this.sps = null;
        this.pps = null;
        this.waitingForKey = true;
        this.failures += 1;
        // "Not supported" won't get better with the next key frame (which an
        // iPhone may not send for a long time): hand over to Media Source now.
        if (error.name === 'NotSupportedError' || this.failures >= 3) this.onFail(String(error));
      },
    });
    // avcC as `description` and length-prefixed frames: the form every
    // engine decodes. Annex B without a description is Chromium's extra, and
    // WebKit's WebCodecs isn't sure to take it.
    decoder.configure({
      codec: codecString(sps),
      description: avcConfig(sps, pps),
      optimizeForLatency: true,
    });
    this.decoder = decoder;
    this.sps = sps;
    this.pps = pps;
  }

  push(key: boolean, pts: number, _data: Uint8Array, nals: Nal[]) {
    if (key) {
      const sets = parameterSets(nals);
      if (sets && (!this.decoder || !equal(this.sps, sets.sps) || !equal(this.pps, sets.pps))) {
        try {
          this.configure(sets.sps, sets.pps);
        } catch (error) {
          this.onFail(String(error));
          return;
        }
      }
      if (this.decoder) this.waitingForKey = false;
    }
    if (this.waitingForKey || !this.decoder || this.decoder.state !== 'configured') return;
    const units = frameUnits(nals);
    if (units.length === 0) return;
    this.latency.received(pts);
    this.decoder.decode(
      new EncodedVideoChunk({ type: key ? 'key' : 'delta', timestamp: pts, data: sample(units) }),
    );
    this.sent += 1;
    if (!this.decodedAny && this.sent >= SILENT_CHUNKS) {
      this.onFail(`${this.sent} chunks, no picture`);
    }
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
  /** Stay closer to the live edge: the source sends steadily. */
  lowLatency = false;

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

  /** Live is live: jump to the newest picture when playback falls behind.
   * A jump decodes from the last key frame on, a stall of its own, so a
   * source that sends steadily (`StreamPlayer.setLowLatency`) is instead
   * played a little faster until it is within a few frames of the edge. */
  private chase() {
    const { buffered } = this.video;
    if (buffered.length === 0) return;
    const end = buffered.end(buffered.length - 1);
    const behind = end - this.video.currentTime;
    if (behind > (this.lowLatency ? 1 : 0.5)) {
      this.video.currentTime = Math.max(0, end - 0.05);
    } else if (this.lowLatency) {
      this.video.playbackRate = behind > 0.15 ? 1.25 : behind > 0.05 ? 1.1 : 1;
    }
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
    // Parameter sets live in the init segment.
    const units = frameUnits(nals);
    if (units.length === 0) return;
    this.sequence += 1;
    this.queue.push(
      mediaSegment(this.sequence, Math.round(this.time), Math.round(duration), sample(units), key),
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

/**
 * Decoded NV12 pictures onto a canvas. `VideoFrame` copies the bytes when it
 * is made, so the picture's memory goes back to Rust right away; drawing it
 * converts to RGB on the graphics card.
 */
class FramesBackend {
  private readonly context: CanvasRenderingContext2D | null;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly onFrame: (width: number, height: number) => void,
    private readonly onFail: (reason: string) => void,
  ) {
    this.context = canvas.getContext('2d', { alpha: false, desynchronized: true });
  }

  draw(raw: RawFrame) {
    let frame: VideoFrame;
    try {
      frame = new VideoFrame(raw.data, {
        format: 'NV12',
        codedWidth: raw.width,
        codedHeight: raw.height,
        timestamp: raw.pts,
      });
    } catch (error) {
      this.onFail(String(error));
      return;
    } finally {
      raw.done();
    }
    const { width, height } = raw;
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width;
      this.canvas.height = height;
    }
    this.context?.drawImage(frame, 0, 0, width, height);
    frame.close();
    this.onFrame(width, height);
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
  /** Set once the stream sends decoded pictures instead of H.264. */
  private frames: FramesBackend | null = null;
  private framesFailed = false;
  private info: PlayerInfo = { backend: 'none', width: 0, height: 0, frames: 0, error: null };
  private readonly listeners = new Set<(info: PlayerInfo) => void>();
  private size = { width: 0, height: 0 };
  private triedMediaSource = false;
  private lowLatency = false;

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
    const backend = new MediaSourceBackend(
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
    backend.lowLatency = this.lowLatency;
    this.backend = backend;
    this.set({ backend: 'mediasource' });
  }

  /** The size the source announced, for engines that need it up front. */
  setSize(width: number, height: number) {
    this.size = { width, height };
  }

  /** The source sends steadily over a local network (UwUCast): Media
   * Source, if it plays, stays closer to the live edge. WebCodecs draws
   * every frame as it comes either way. */
  setLowLatency(on: boolean) {
    this.lowLatency = on;
    if (this.backend instanceof MediaSourceBackend) this.backend.lowLatency = on;
  }

  push(key: boolean, pts: number, data: Uint8Array) {
    if (!this.backend) return;
    this.backend.push(key, pts, data, splitNals(data));
  }

  /** A decoded picture: the H.264 decoder isn't needed for this stream. */
  pushFrame(raw: RawFrame) {
    if (!this.frames) {
      if (this.framesFailed || typeof window.VideoFrame !== 'function') {
        raw.done();
        if (!this.framesFailed) {
          this.framesFailed = true;
          this.set({ backend: 'none', error: 'no-decoder' });
        }
        return;
      }
      this.backend?.close();
      this.backend = null;
      const canvas = document.createElement('canvas');
      this.element.replaceChildren(canvas);
      this.frames = new FramesBackend(canvas, this.frame, (reason) => {
        console.warn('VideoFrame refused the picture:', reason);
        this.frames = null;
        this.framesFailed = true;
        this.set({ backend: 'none', error: 'no-decoder' });
      });
      this.set({ backend: 'frames' });
    }
    this.frames.draw(raw);
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
    this.frames = null;
    this.listeners.clear();
    this.element.remove();
  }
}
