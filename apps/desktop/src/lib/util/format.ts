// Formatting for the UI. Dates follow the owner's style: "14 Jul 2023".

const LOCALE = 'en-GB';

const num = new Intl.NumberFormat(LOCALE);
export const n = (v: number) => num.format(v);

export function plural(count: number, one: string, many = `${one}s`) {
  return `${n(count)} ${count === 1 ? one : many}`;
}

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const MON = MONTHS.map((m) => m.slice(0, 3));
const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

export const monthName = (m: number) => MONTHS[m - 1] ?? '';
export const monthShort = (m: number) => MON[m - 1] ?? '';

/** yyyymmdd → parts */
export function splitDay(day: number) {
  return { y: Math.floor(day / 10000), m: Math.floor(day / 100) % 100, d: day % 100 };
}

export function dayLabel(day: number, withYear = false) {
  if (!day) return 'No date';
  const { y, m, d } = splitDay(day);
  const wd = DAYS[new Date(Date.UTC(y, m - 1, d)).getUTCDay()];
  return `${wd} ${d} ${monthName(m)}${withYear ? ` ${y}` : ''}`;
}

/** "Tue 30 Dec" for narrow headers. */
export function dayShort(day: number) {
  if (!day) return 'No date';
  const { y, m, d } = splitDay(day);
  const wd = DAYS[new Date(Date.UTC(y, m - 1, d)).getUTCDay()]!.slice(0, 3);
  return `${wd} ${d} ${monthShort(m)}`;
}

export function monthLabel(day: number) {
  if (!day) return 'No date';
  const { y, m } = splitDay(day);
  return `${monthName(m)} ${y}`;
}

/** "2019-08-12T14:03:22" → "12 Aug 2019" */
export function shortDate(local: string | null | undefined) {
  if (!local) return '';
  const y = local.slice(0, 4);
  const m = Number(local.slice(5, 7));
  const d = Number(local.slice(8, 10));
  return `${d} ${monthShort(m)} ${y}`;
}

export function time(local: string | null | undefined) {
  return local ? local.slice(11, 16) : '';
}

export function offset(min: number | null | undefined) {
  if (min == null) return '';
  const s = min < 0 ? '−' : '+';
  const a = Math.abs(min);
  return `${s}${Math.floor(a / 60)}:${String(a % 60).padStart(2, '0')}`;
}

export function bytes(b: number) {
  if (b < 1024) return `${b} B`;
  const u = ['KB', 'MB', 'GB', 'TB'];
  let v = b / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${u[i]}`;
}

export function duration(ms: number | null | undefined) {
  if (ms == null) return '';
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

export function eta(secs: number | null | undefined) {
  if (secs == null) return '';
  if (secs < 60) return 'under a minute';
  if (secs < 3600) return `${Math.round(secs / 60)} min`;
  return `${Math.floor(secs / 3600)} h ${Math.round((secs % 3600) / 60)} min`;
}

export function compact(v: number) {
  if (v < 1000) return String(v);
  if (v < 10000) return `${(v / 1000).toFixed(1)}k`;
  return `${Math.round(v / 1000)}k`;
}

export function coords(lat: number, lon: number) {
  return `${Math.abs(lat).toFixed(5)}° ${lat >= 0 ? 'N' : 'S'}, ${Math.abs(lon).toFixed(5)}° ${lon >= 0 ? 'E' : 'W'}`;
}

export function accuracy(m: number | null | undefined) {
  if (m == null) return '';
  if (m < 1000) return `±${Math.round(m)} m`;
  return `±${Math.round(m / 1000)} km`;
}
