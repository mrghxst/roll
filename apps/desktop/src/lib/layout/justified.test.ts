import { describe, expect, it } from 'vitest';
import { firstVisible, layout, sectionAt, DAY_HEADER, MONTH_HEADER } from './justified';

const arr = (n: number, v: number) => new Float32Array(n).fill(v);

describe('justified layout', () => {
  it('fills full rows to the width and keeps the last row at target height', () => {
    const l = layout({ aspects: arr(7, 1.5), days: new Uint32Array(7).fill(20190812), width: 1000, rowHeight: 200, padX: 0 });
    // 3 tiles of 1.5 at 200 px = 900 + gaps < 1000; the 4th overflows.
    const firstRow = [0, 1, 2, 3].map((i) => l.y[i]);
    expect(new Set(firstRow).size).toBe(1);
    expect((l.x[3] ?? 0) + (l.w[3] ?? 0)).toBeCloseTo(1000, 0);
    expect(l.h[0]).toBeLessThan(200);
    expect(l.h[6]).toBeCloseTo(200, 3);
  });

  it('puts short days side by side with their own headers', () => {
    const days = new Uint32Array([20190813, 20190812, 20190812, 20190712]);
    const l = layout({ aspects: arr(4, 1), days, width: 1000, rowHeight: 100, padX: 0 });
    expect(l.sections.map((s) => s.kind)).toEqual(['month', 'day', 'day', 'month', 'day']);
    const [, d13, d12] = l.sections;
    expect(d13!.y).toBe(d12!.y);
    expect(d12!.x).toBeGreaterThan(d13!.x);
    expect(l.y[0]).toBe(MONTH_HEADER + DAY_HEADER);
    expect(l.y[1]).toBe(l.y[0]);
    const july = l.sections[3]!;
    expect(july.day).toBe(20190712);
    expect(july.y).toBeGreaterThan(l.y[0]!);
    expect(sectionAt(l.sections, july.y + 5, 'month')).toBe(july);
  });

  it('keeps tiles sorted by y and finds the first visible one', () => {
    const days = new Uint32Array(300);
    for (let i = 0; i < 300; i++) days[i] = 20200131 - Math.floor(i / 3);
    const l = layout({ aspects: arr(300, 1), days, width: 700, rowHeight: 100 });
    for (let i = 1; i < 300; i++) expect(l.y[i]!).toBeGreaterThanOrEqual(l.y[i - 1]!);
    const i = firstVisible(l, 1000);
    expect((l.y[i] ?? 0) + (l.h[i] ?? 0)).toBeGreaterThanOrEqual(1000);
    expect((l.y[i - 1] ?? 0) + (l.h[i - 1] ?? 0)).toBeLessThan(1000);
  });

  it('lays out 100k items quickly', () => {
    const n = 100_000;
    const days = new Uint32Array(n);
    for (let i = 0; i < n; i++) days[i] = 20250101 - Math.floor(i / 40);
    const t = performance.now();
    const l = layout({ aspects: arr(n, 1.33), days, width: 1600, rowHeight: 200 });
    expect(performance.now() - t).toBeLessThan(250);
    expect(l.height).toBeGreaterThan(0);
  });
});
