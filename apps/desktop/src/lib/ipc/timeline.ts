// Decodes the packed timeline payload from the backend (see
// crates/core/src/query.rs): header (magic, version, count), then
// ids u32, days u32 (yyyymmdd), aspect f32, color u32, flags u16,
// thumbnail version u16.

export const MAGIC = 0x4c4c4f52;

export const Flag = {
  VIDEO: 1,
  LIVE: 2,
  BURST: 4,
  RAW: 8,
  FAVORITE: 16,
  PROBLEM: 32,
  IMMICH: 64,
  PRIVATE: 128,
  EDITED: 256,
  NO_THUMB: 512,
  /** Map points only: in Photos, preferred for a cluster's thumbnail. */
  PHOTO: 1024,
} as const;

export interface Timeline {
  count: number;
  ids: Uint32Array;
  days: Uint32Array;
  aspects: Float32Array;
  colors: Uint32Array;
  flags: Uint16Array;
  versions: Uint16Array;
}

export function emptyTimeline(): Timeline {
  return {
    count: 0,
    ids: new Uint32Array(0),
    days: new Uint32Array(0),
    aspects: new Float32Array(0),
    colors: new Uint32Array(0),
    flags: new Uint16Array(0),
    versions: new Uint16Array(0),
  };
}

export function decodeTimeline(buf: ArrayBuffer): Timeline {
  const head = new DataView(buf, 0, 12);
  if (head.getUint32(0, true) !== MAGIC) throw new Error('Unexpected timeline data');
  const count = head.getUint32(8, true);
  let off = 12;
  const take = <T>(ctor: new (b: ArrayBuffer, o: number, n: number) => T, size: number): T => {
    const arr = new ctor(buf, off, count);
    off += count * size;
    return arr;
  };
  // Typed arrays need aligned offsets; the payload keeps 4-byte arrays first.
  return {
    count,
    ids: take(Uint32Array, 4),
    days: take(Uint32Array, 4),
    aspects: take(Float32Array, 4),
    colors: take(Uint32Array, 4),
    flags: take(Uint16Array, 2),
    versions: take(Uint16Array, 2),
  };
}

export function encodeTimeline(t: Omit<Timeline, 'count'>): ArrayBuffer {
  const n = t.ids.length;
  const buf = new ArrayBuffer(12 + n * 20);
  const dv = new DataView(buf);
  dv.setUint32(0, MAGIC, true);
  dv.setUint32(4, 1, true);
  dv.setUint32(8, n, true);
  let off = 12;
  new Uint32Array(buf, off, n).set(t.ids);
  off += n * 4;
  new Uint32Array(buf, off, n).set(t.days);
  off += n * 4;
  new Float32Array(buf, off, n).set(t.aspects);
  off += n * 4;
  new Uint32Array(buf, off, n).set(t.colors);
  off += n * 4;
  new Uint16Array(buf, off, n).set(t.flags);
  off += n * 2;
  new Uint16Array(buf, off, n).set(t.versions);
  return buf;
}

export function colorCss(rgb: number): string {
  if (!rgb) return 'var(--surface)';
  return `rgb(${(rgb >> 16) & 255} ${(rgb >> 8) & 255} ${rgb & 255})`;
}
