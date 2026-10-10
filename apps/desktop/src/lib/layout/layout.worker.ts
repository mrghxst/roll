/// <reference lib="webworker" />
import { layout, type LayoutInput } from './justified';

self.onmessage = (e: MessageEvent<LayoutInput & { seq: number }>) => {
  const { seq, ...input } = e.data;
  const l = layout(input);
  (self as unknown as Worker).postMessage({ seq, layout: l }, [l.x.buffer, l.y.buffer, l.w.buffer, l.h.buffer]);
};
