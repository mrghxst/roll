import { describe, expect, it } from 'vitest';
import { decodeMapPoints, encodeMapPoints } from './mappoints';

describe('map payload', () => {
  it('round-trips an odd count (padded flags)', () => {
    const p = {
      count: 3,
      ids: new Uint32Array([1, 2, 3]),
      days: new Uint32Array([20190803, 20190804, 20220716]),
      lats: new Float32Array([45.88, 45.9, 46.6]),
      lons: new Float32Array([10.84, 10.85, 11.72]),
      accs: new Float32Array([10, 0, 200000]),
      flags: new Uint16Array([0, 1, 16]),
    };
    const q = decodeMapPoints(encodeMapPoints(p));
    expect(q.count).toBe(3);
    expect([...q.ids]).toEqual([1, 2, 3]);
    expect(q.lats[2]).toBeCloseTo(46.6, 4);
    expect([...q.flags]).toEqual([0, 1, 16]);
    expect(q.accs[2]).toBe(200000);
  });
});
