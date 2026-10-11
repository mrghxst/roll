// UTC offsets offered in date tools, in minutes east of UTC.

import { offsetLabel } from '../state/places';

const HALF = [-570, -210, 210, 270, 330, 345, 390, 525, 570, 630, 765, 825];

export const OFFSETS: { value: number; label: string }[] = [...Array.from({ length: 27 }, (_, i) => (i - 12) * 60), ...HALF]
  .sort((a, b) => a - b)
  .map((m) => ({ value: m, label: offsetLabel(m) }));

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** "3 Aug 2019 09:08" from "2019-08-03T09:08:28". */
export function shortLocal(s: string | null | undefined): string {
  if (!s) return 'no date';
  return `${Number(s.slice(8, 10))} ${MONTHS[Number(s.slice(5, 7)) - 1]} ${s.slice(0, 4)} ${s.slice(11, 16)}`;
}
