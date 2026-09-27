/**
 * Fragmented MP4 for Media Source, the fallback when the webview has no
 * WebCodecs (WebKitGTK on some Linux distributions): an init segment with the
 * SPS and PPS, then one moof + mdat per frame. Just the boxes a browser needs
 * for a single H.264 video track, nothing more.
 */

export const TIMESCALE = 90_000;

function u32(value: number): number[] {
  return [(value >>> 24) & 0xff, (value >>> 16) & 0xff, (value >>> 8) & 0xff, value & 0xff];
}

function u16(value: number): number[] {
  return [(value >>> 8) & 0xff, value & 0xff];
}

function u64(value: number): number[] {
  const high = Math.floor(value / 2 ** 32);
  return [...u32(high), ...u32(value >>> 0)];
}

function ascii(text: string): number[] {
  return [...text].map((c) => c.charCodeAt(0));
}

type Part = Uint8Array | number[];
type Bytes = Uint8Array<ArrayBuffer>;

function concat(parts: Part[]): Bytes {
  const length = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(length);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}

function box(type: string, ...parts: Part[]): Bytes {
  const body = concat(parts);
  return concat([u32(body.length + 8), ascii(type), body]);
}

function fullBox(type: string, version: number, flags: number, ...parts: Part[]): Bytes {
  return box(type, [version, (flags >> 16) & 0xff, (flags >> 8) & 0xff, flags & 0xff], ...parts);
}

const MATRIX = [
  ...u32(0x10000),
  ...u32(0),
  ...u32(0),
  ...u32(0),
  ...u32(0x10000),
  ...u32(0),
  ...u32(0),
  ...u32(0),
  ...u32(0x40000000),
];

/** The body of an avcC box: one SPS, one PPS, 4-byte lengths. WebCodecs
 * takes the same bytes as a decoder's `description`. */
export function avcConfig(sps: Uint8Array, pps: Uint8Array): Bytes {
  return concat([
    [1, sps[1] ?? 0x64, sps[2] ?? 0, sps[3] ?? 0x28, 0xff, 0xe1],
    u16(sps.length),
    sps,
    [1],
    u16(pps.length),
    pps,
  ]);
}

/** ftyp + moov for one H.264 track of the given size. */
export function initSegment(
  sps: Uint8Array,
  pps: Uint8Array,
  width: number,
  height: number,
): Bytes {
  const avcC = box('avcC', avcConfig(sps, pps));
  const avc1 = box(
    'avc1',
    [0, 0, 0, 0, 0, 0, ...u16(1)],
    new Array(16).fill(0),
    u16(width),
    u16(height),
    u32(0x480000),
    u32(0x480000),
    u32(0),
    u16(1),
    new Array(32).fill(0),
    u16(0x18),
    u16(0xffff),
    avcC,
  );
  const stbl = box(
    'stbl',
    fullBox('stsd', 0, 0, u32(1), avc1),
    fullBox('stts', 0, 0, u32(0)),
    fullBox('stsc', 0, 0, u32(0)),
    fullBox('stsz', 0, 0, u32(0), u32(0)),
    fullBox('stco', 0, 0, u32(0)),
  );
  const minf = box(
    'minf',
    fullBox('vmhd', 0, 1, u16(0), u16(0), u16(0), u16(0)),
    box('dinf', fullBox('dref', 0, 0, u32(1), fullBox('url ', 0, 1))),
    stbl,
  );
  const mdia = box(
    'mdia',
    fullBox('mdhd', 0, 0, u32(0), u32(0), u32(TIMESCALE), u32(0), u16(0x55c4), u16(0)),
    fullBox('hdlr', 0, 0, u32(0), ascii('vide'), u32(0), u32(0), u32(0), ascii('Video'), [0]),
    minf,
  );
  const tkhd = fullBox(
    'tkhd',
    0,
    3,
    u32(0),
    u32(0),
    u32(1),
    u32(0),
    u32(0),
    u32(0),
    u32(0),
    u16(0),
    u16(0),
    u16(0),
    u16(0),
    MATRIX,
    u32(width * 0x10000),
    u32(height * 0x10000),
  );
  const mvhd = fullBox(
    'mvhd',
    0,
    0,
    u32(0),
    u32(0),
    u32(TIMESCALE),
    u32(0),
    u32(0x10000),
    u16(0x100),
    new Array(10).fill(0),
    MATRIX,
    new Array(24).fill(0),
    u32(2),
  );
  const mvex = box('mvex', fullBox('trex', 0, 0, u32(1), u32(1), u32(0), u32(0), u32(0)));
  return concat([
    box('ftyp', ascii('isom'), u32(0x200), ascii('isomiso2avc1mp41')),
    box('moov', mvhd, box('trak', tkhd, mdia), mvex),
  ]);
}

/** Annex B NAL units → one length-prefixed MP4 sample. */
export function sample(nals: Uint8Array[]): Bytes {
  return concat(nals.flatMap((nal) => [u32(nal.length), nal]));
}

/** moof + mdat carrying one sample. `time` and `duration` are in TIMESCALE units. */
export function mediaSegment(
  sequence: number,
  time: number,
  duration: number,
  data: Uint8Array,
  key: boolean,
): Bytes {
  // A key frame depends on nothing; anything else depends on others and is
  // no place to start from.
  const flags = key ? 0x02000000 : 0x01010000;
  const trun = (offset: number) =>
    fullBox('trun', 0, 0x000701, u32(1), u32(offset), u32(duration), u32(data.length), u32(flags));
  const build = (offset: number) =>
    box(
      'moof',
      fullBox('mfhd', 0, 0, u32(sequence)),
      box(
        'traf',
        fullBox('tfhd', 0, 0x020000, u32(1)),
        fullBox('tfdt', 1, 0, u64(time)),
        trun(offset),
      ),
    );
  // The data offset counts from the start of moof to the first sample byte.
  const moofLength = build(0).length;
  return concat([build(moofLength + 8), box('mdat', data)]);
}
