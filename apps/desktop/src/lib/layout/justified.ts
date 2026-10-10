// Justified rows grouped by month and day. Short days share a row, each
// with its own small header; longer days get full justified rows. Pure and
// allocation-light so it lays out 100k items in a few milliseconds.

export interface LayoutInput {
  aspects: Float32Array;
  days: Uint32Array;
  width: number;
  rowHeight: number;
  gap?: number;
  padX?: number;
  padRight?: number;
  /** Group by day with headers (timeline) or one block (search results). */
  headers?: boolean;
}

export const MONTH_HEADER = 52;
export const DAY_HEADER = 36;
/** Space between days sharing a row. */
const DAY_GAP = 16;

export interface Section {
  kind: 'month' | 'day';
  day: number;
  x: number;
  y: number;
  w: number;
  /** First item index and one past the last. */
  start: number;
  end: number;
}

export interface Layout {
  x: Float32Array;
  y: Float32Array;
  w: Float32Array;
  h: Float32Array;
  sections: Section[];
  height: number;
  width: number;
}

const month = (day: number) => Math.floor(day / 100);

export function layout(input: LayoutInput): Layout {
  const { aspects, days, rowHeight } = input;
  const gap = input.gap ?? 2;
  const padX = input.padX ?? 16;
  const padRight = input.padRight ?? padX;
  const headers = input.headers ?? true;
  const n = aspects.length;
  const inner = Math.max(100, input.width - padX - padRight);
  const x = new Float32Array(n);
  const y = new Float32Array(n);
  const w = new Float32Array(n);
  const h = new Float32Array(n);
  const sections: Section[] = [];
  let cy = headers ? 0 : 8;

  /** Lay out [from, to) as one row at height `rh`, starting at `left`. */
  const placeRow = (from: number, to: number, rh: number, left: number, top: number) => {
    let cx = left;
    for (let i = from; i < to; i++) {
      const iw = (aspects[i] ?? 1) * rh;
      x[i] = cx;
      y[i] = top;
      w[i] = iw;
      h[i] = rh;
      cx += iw + gap;
    }
  };

  /** Full-width justified rows for [from, to). */
  const justify = (from: number, to: number) => {
    let rowStart = from;
    let sumA = 0;
    for (let k = from; k < to; k++) {
      sumA += aspects[k] ?? 1;
      const count = k - rowStart + 1;
      if (sumA * rowHeight + gap * (count - 1) >= inner) {
        const rh = (inner - gap * (count - 1)) / sumA;
        placeRow(rowStart, k + 1, rh, padX, cy);
        // Snap the last tile to the edge to hide rounding gaps.
        w[k] = padX + inner - (x[k] ?? 0);
        cy += rh + gap;
        rowStart = k + 1;
        sumA = 0;
      }
    }
    if (rowStart < to) {
      placeRow(rowStart, to, rowHeight, padX, cy);
      cy += rowHeight + gap;
    }
  };

  if (!headers) {
    justify(0, n);
    return { x, y, w, h, sections, height: cy + 48, width: input.width };
  }

  // A band holds several short days side by side.
  let bandX = -1;
  let monthSection: Section | null = null;
  let i = 0;
  while (i < n) {
    const day = days[i] ?? 0;
    let j = i;
    let sumA = 0;
    while (j < n && days[j] === day) sumA += aspects[j++] ?? 1;
    const naturalW = sumA * rowHeight + gap * (j - i - 1);

    if (!monthSection || month(day) !== month(monthSection.day)) {
      if (bandX >= 0) {
        cy += DAY_HEADER + rowHeight + gap + 8;
        bandX = -1;
      }
      if (monthSection) monthSection.end = i;
      monthSection = { kind: 'month', day, x: padX, y: cy, w: inner, start: i, end: n };
      sections.push(monthSection);
      cy += MONTH_HEADER;
    }

    if (naturalW < inner) {
      // A short day: join the current band if it fits.
      if (bandX >= 0 && bandX + DAY_GAP + naturalW <= padX + inner) {
        bandX += DAY_GAP;
      } else {
        if (bandX >= 0) cy += DAY_HEADER + rowHeight + gap + 8;
        bandX = padX;
      }
      sections.push({ kind: 'day', day, x: bandX, y: cy, w: naturalW, start: i, end: j });
      placeRow(i, j, rowHeight, bandX, cy + DAY_HEADER);
      bandX += naturalW;
    } else {
      if (bandX >= 0) {
        cy += DAY_HEADER + rowHeight + gap + 8;
        bandX = -1;
      }
      sections.push({ kind: 'day', day, x: padX, y: cy, w: inner, start: i, end: j });
      cy += DAY_HEADER;
      justify(i, j);
      cy += 8;
    }
    i = j;
  }
  if (bandX >= 0) cy += DAY_HEADER + rowHeight + gap + 8;
  return { x, y, w, h, sections, height: cy + 48, width: input.width };
}

/**
 * First index whose tile bottom is at or below `top`. Tiles in a band share
 * a y, so the array is sorted by y and binary search works.
 */
export function firstVisible(l: Layout, top: number): number {
  let lo = 0;
  let hi = l.y.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((l.y[mid] ?? 0) + (l.h[mid] ?? 0) < top) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** The section (month or day) at a scroll position. */
export function sectionAt(sections: Section[], top: number, kind: 'month' | 'day'): Section | null {
  let found: Section | null = null;
  for (const s of sections) {
    if (s.kind !== kind) continue;
    if (s.y <= top + 1) found = s;
    else break;
  }
  return found;
}
