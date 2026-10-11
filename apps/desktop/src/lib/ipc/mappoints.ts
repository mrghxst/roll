// Decodes the packed map payload (see `map_points` in
// crates/core/src/query.rs): header (magic, version, count), then
// ids u32, days u32 (yyyymmdd), lat f32, lon f32, accuracy f32 (metres),
// flags u16 padded to 4 bytes. Oldest first.

export const MAP_MAGIC = 0x5050414d;

export interface MapPoints {
  count: number;
  ids: Uint32Array;
  days: Uint32Array;
  lats: Float32Array;
  lons: Float32Array;
  accs: Float32Array;
  flags: Uint16Array;
}

export function emptyMapPoints(): MapPoints {
  return {
    count: 0,
    ids: new Uint32Array(0),
    days: new Uint32Array(0),
    lats: new Float32Array(0),
    lons: new Float32Array(0),
    accs: new Float32Array(0),
    flags: new Uint16Array(0),
  };
}

export function decodeMapPoints(buf: ArrayBuffer): MapPoints {
  const head = new Uint32Array(buf, 0, 3);
  if (head[0] !== MAP_MAGIC) throw new Error('Unexpected map payload');
  const n = head[2]!;
  let off = 12;
  const u32 = () => {
    const a = new Uint32Array(buf, off, n);
    off += n * 4;
    return a;
  };
  const f32 = () => {
    const a = new Float32Array(buf, off, n);
    off += n * 4;
    return a;
  };
  const ids = u32();
  const days = u32();
  const lats = f32();
  const lons = f32();
  const accs = f32();
  const flags = new Uint16Array(buf, off, n);
  return { count: n, ids, days, lats, lons, accs, flags };
}

export function encodeMapPoints(p: MapPoints): ArrayBuffer {
  const n = p.count;
  const pad = n % 2 === 1 ? 2 : 0;
  const buf = new ArrayBuffer(12 + n * 22 + pad);
  new Uint32Array(buf, 0, 3).set([MAP_MAGIC, 1, n]);
  let off = 12;
  for (const a of [p.ids, p.days]) {
    new Uint32Array(buf, off, n).set(a);
    off += n * 4;
  }
  for (const a of [p.lats, p.lons, p.accs]) {
    new Float32Array(buf, off, n).set(a);
    off += n * 4;
  }
  new Uint16Array(buf, off, n).set(p.flags);
  return buf;
}
