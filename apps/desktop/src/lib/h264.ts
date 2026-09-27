/**
 * Just enough H.264 to feed a decoder: splitting Annex B into NAL units, the
 * codec string for WebCodecs and Media Source, and the picture size from an
 * SPS (for the MP4 track header of the Media Source path).
 */

export type Nal = { type: number; data: Uint8Array };

/** The NAL units of an Annex B access unit, without their start codes. */
export function splitNals(data: Uint8Array): Nal[] {
  const starts: [at: number, codeLength: number][] = [];
  for (let i = 0; i + 2 < data.length; i++) {
    if (data[i] === 0 && data[i + 1] === 0) {
      if (data[i + 2] === 1) {
        starts.push([i, 3]);
        i += 2;
      } else if (data[i + 2] === 0 && data[i + 3] === 1) {
        starts.push([i, 4]);
        i += 3;
      }
    }
  }
  const nals: Nal[] = [];
  starts.forEach(([at, code], index) => {
    const begin = at + code;
    const end = index + 1 < starts.length ? starts[index + 1]![0] : data.length;
    if (end > begin) {
      const nal = data.subarray(begin, end);
      nals.push({ type: nal[0]! & 0x1f, data: nal });
    }
  });
  return nals;
}

function hex(byte: number): string {
  return byte.toString(16).padStart(2, '0');
}

/** "avc1.PPCCLL" from the SPS: profile, constraint flags, level. */
export function codecString(sps: Uint8Array): string {
  return `avc1.${hex(sps[1] ?? 0x64)}${hex(sps[2] ?? 0)}${hex(sps[3] ?? 0x28)}`;
}

/** The SPS and PPS in an access unit, if it carries them. */
export function parameterSets(nals: Nal[]): { sps: Uint8Array; pps: Uint8Array } | null {
  const sps = nals.find((nal) => nal.type === 7);
  const pps = nals.find((nal) => nal.type === 8);
  return sps && pps ? { sps: sps.data, pps: pps.data } : null;
}

/** Reads bits MSB first from an RBSP (emulation prevention already removed). */
class Bits {
  private bit = 0;
  constructor(private readonly bytes: Uint8Array) {}

  u(count: number): number {
    let value = 0;
    for (let i = 0; i < count; i++) {
      const byte = this.bytes[this.bit >> 3];
      if (byte === undefined) throw new Error('SPS ends early');
      value = value * 2 + ((byte >> (7 - (this.bit & 7))) & 1);
      this.bit++;
    }
    return value;
  }

  ue(): number {
    let zeros = 0;
    while (this.u(1) === 0) {
      zeros++;
      if (zeros > 31) throw new Error('bad Exp-Golomb code');
    }
    return 2 ** zeros - 1 + this.u(zeros);
  }

  se(): number {
    const value = this.ue();
    return value % 2 === 0 ? -value / 2 : (value + 1) / 2;
  }
}

function unescape(nal: Uint8Array): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < nal.length; i++) {
    if (i >= 2 && nal[i] === 3 && nal[i - 1] === 0 && nal[i - 2] === 0) continue;
    out.push(nal[i]!);
  }
  return Uint8Array.from(out);
}

const HIGH_PROFILES = new Set([100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135]);

/** Width and height of the picture an SPS describes, after cropping. */
export function spsSize(sps: Uint8Array): { width: number; height: number } | null {
  try {
    const bits = new Bits(unescape(sps.subarray(1)));
    const profile = bits.u(8);
    bits.u(16); // constraint flags, level
    bits.ue(); // seq_parameter_set_id
    let chroma = 1;
    if (HIGH_PROFILES.has(profile)) {
      chroma = bits.ue();
      if (chroma === 3) bits.u(1);
      bits.ue();
      bits.ue();
      bits.u(1);
      if (bits.u(1)) {
        for (let i = 0; i < (chroma === 3 ? 12 : 8); i++) {
          if (!bits.u(1)) continue;
          const size = i < 6 ? 16 : 64;
          let last = 8;
          let next = 8;
          for (let j = 0; j < size; j++) {
            if (next !== 0) next = (last + bits.se() + 256) % 256;
            last = next === 0 ? last : next;
          }
        }
      }
    }
    bits.ue(); // log2_max_frame_num_minus4
    const pocType = bits.ue();
    if (pocType === 0) {
      bits.ue();
    } else if (pocType === 1) {
      bits.u(1);
      bits.se();
      bits.se();
      const cycle = bits.ue();
      for (let i = 0; i < cycle; i++) bits.se();
    }
    bits.ue(); // max_num_ref_frames
    bits.u(1);
    const widthMbs = bits.ue() + 1;
    const heightMaps = bits.ue() + 1;
    const frameMbsOnly = bits.u(1);
    if (!frameMbsOnly) bits.u(1);
    bits.u(1);
    let crop = [0, 0, 0, 0];
    if (bits.u(1)) crop = [bits.ue(), bits.ue(), bits.ue(), bits.ue()];
    const unitX = chroma === 1 || chroma === 2 ? 2 : 1;
    const unitY = (chroma === 1 ? 2 : 1) * (2 - frameMbsOnly);
    return {
      width: widthMbs * 16 - (crop[0]! + crop[1]!) * unitX,
      height: (2 - frameMbsOnly) * heightMaps * 16 - (crop[2]! + crop[3]!) * unitY,
    };
  } catch {
    return null;
  }
}
