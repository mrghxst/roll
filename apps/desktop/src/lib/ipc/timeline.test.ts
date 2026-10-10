import { describe, expect, it } from 'vitest';
import { decodeTimeline, encodeTimeline } from './timeline';

describe('timeline payload', () => {
  it('round-trips', () => {
    const t = {
      ids: new Uint32Array([3, 2, 1]),
      days: new Uint32Array([20190813, 20190812, 0]),
      aspects: new Float32Array([1.5, 0.75, 1]),
      colors: new Uint32Array([0xff0000, 0, 0x00ff00]),
      flags: new Uint16Array([1, 0, 16]),
      versions: new Uint16Array([1, 2, 3]),
    };
    const d = decodeTimeline(encodeTimeline(t));
    expect(d.count).toBe(3);
    expect([...d.ids]).toEqual([3, 2, 1]);
    expect([...d.days]).toEqual([20190813, 20190812, 0]);
    expect(d.aspects[1]).toBeCloseTo(0.75);
    expect([...d.flags]).toEqual([1, 0, 16]);
    expect([...d.versions]).toEqual([1, 2, 3]);
  });
});
