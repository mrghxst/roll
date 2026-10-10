// Can the WebView play this video directly, or does it need a proxy?

const CODEC_TYPES: Record<string, string> = {
  h264: 'video/mp4; codecs="avc1.640028"',
  hevc: 'video/mp4; codecs="hvc1.1.6.L93.B0"',
  av1: 'video/mp4; codecs="av01.0.05M.08"',
  vp9: 'video/webm; codecs="vp9"',
  vp8: 'video/webm; codecs="vp8"',
};

const cache = new Map<string, boolean>();

export function playable(format: string, codec: string | null): boolean {
  const key = `${format}:${codec}`;
  const hit = cache.get(key);
  if (hit !== undefined) return hit;
  let ok = false;
  const type = codec ? CODEC_TYPES[codec] : undefined;
  if (type && typeof document !== 'undefined') {
    const v = document.createElement('video');
    const ms = typeof MediaSource !== 'undefined' && MediaSource.isTypeSupported(type);
    ok = ms || v.canPlayType(type) === 'probably';
  } else if (!codec && (format === 'mp4' || format === 'webm')) {
    ok = true;
  }
  cache.set(key, ok);
  return ok;
}
