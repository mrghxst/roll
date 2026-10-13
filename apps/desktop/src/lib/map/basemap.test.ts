import { describe, expect, it } from 'vitest';
import { unwrap } from './basemap';

describe('unwrap', () => {
  it('continues a ring across the 180° meridian', () => {
    // Chukotka-like: east of 170°E, across 180°, back.
    const ring = [
      [170, 65],
      [179, 66],
      [-179, 67],
      [-175, 66],
      [-179, 64],
      [179, 64],
      [170, 65],
    ];
    expect(unwrap(ring).map((p) => p[0])).toEqual([170, 179, 181, 185, 181, 179, 170]);
  });

  it('leaves rings that do not cross it, and rings around a pole', () => {
    const plain = [
      [0, 0],
      [10, 0],
      [10, 10],
      [0, 0],
    ];
    expect(unwrap(plain)).toEqual(plain);
    const pole = [
      [-180, -80],
      [0, -75],
      [180, -80],
      [180, -90],
      [-180, -90],
      [-180, -80],
    ];
    expect(unwrap(pole)).toBe(pole);
  });
});
