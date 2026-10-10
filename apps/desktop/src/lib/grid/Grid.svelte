<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import { thumbHashToDataURL } from 'thumbhash';
  import { Check } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { Query } from '../ipc/bindings/Query';
  import { colorCss, emptyTimeline, type Timeline } from '../ipc/timeline';
  import { firstVisible, sectionAt, type Layout } from '../layout/justified';
  import { app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { viewer } from '../state/viewer.svelte';
  import { dayLabel, dayShort, monthLabel, n } from '../util/format';
  import Scrubber from './Scrubber.svelte';
  import Tile from './Tile.svelte';

  let {
    query,
    headers = true,
    onopen,
    onquicklook,
    empty,
    reloadKey = 0,
  }: {
    query: Query;
    headers?: boolean;
    onopen: (ids: Uint32Array, index: number, rect: DOMRect | null) => void;
    onquicklook?: (ids: Uint32Array, index: number) => void;
    empty?: Snippet;
    reloadKey?: number;
  } = $props();

  const OVERSCAN = 800;
  const rowHeight = $derived(app.settings?.row_height ?? 200);

  let data = $state<Timeline>(emptyTimeline());
  let loaded = $state(false);
  let L = $state<Layout | null>(null);
  let el: HTMLDivElement | undefined = $state();
  let width = $state(0);
  let vh = $state(0);
  let scrollTop = $state(0);
  let slots = $state<number[]>([]);
  let slotOf = new Map<number, number>();
  let focus = $state(-1);
  let band = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  let hashes = $state(new Map<number, string>());
  let durations = $state(new Map<number, number>());

  // Load the payload whenever the query or library changes.
  let loadSeq = 0;
  $effect(() => {
    const q = $state.snapshot(query) as Query;
    void reloadKey;
    void app.revision;
    void app.thumbsRevision;
    const my = ++loadSeq;
    void api.timeline(q).then((t) => {
      if (my !== loadSeq) return;
      data = t;
      loaded = true;
    });
  });

  // Layout in a worker.
  const worker = new Worker(new URL('../layout/layout.worker.ts', import.meta.url), { type: 'module' });
  let layoutSeq = 0;
  let anchor: { id: number; offset: number } | null = null;
  let layoutIds: Uint32Array = new Uint32Array(0);
  worker.onmessage = (e: MessageEvent<{ seq: number; layout: Layout }>) => {
    if (e.data.seq !== layoutSeq) return;
    L = e.data.layout;
    slots = [];
    slotOf = new Map();
    if (anchor && el) {
      const i = data.ids.indexOf(anchor.id);
      if (i >= 0 && i < L.y.length) {
        el.scrollTop = (L.y[i] ?? 0) - anchor.offset;
        scrollTop = el.scrollTop;
      }
    }
    anchor = null;
    layoutIds = data.ids;
    refreshVisible();
  };
  $effect(() => () => worker.terminate());

  $effect(() => {
    if (width <= 0) return;
    const d = data;
    // Keep the item at the top in place when the layout changes.
    untrack(() => {
      // Anchor by id: new items may have been inserted above.
      if (L && el && el.scrollTop > 0 && L.y.length) {
        const i = firstVisible(L, el.scrollTop);
        const id = layoutIds[i];
        if (id !== undefined) anchor = { id, offset: (L.y[i] ?? 0) - el.scrollTop };
      }
    });
    worker.postMessage({
      seq: ++layoutSeq,
      aspects: d.aspects.slice(),
      days: d.days.slice(),
      width,
      rowHeight,
      headers,
      padX: 16,
      padRight: headers ? 48 : 16,
    });
  });

  function refreshVisible() {
    if (!L) return;
    const top = scrollTop - OVERSCAN;
    const bottom = scrollTop + vh + OVERSCAN;
    const first = firstVisible(L, top);
    let last = first;
    while (last < L.y.length && (L.y[last] ?? 0) < bottom) last++;
    const next = slots.slice();
    const free: number[] = [];
    for (let s = 0; s < next.length; s++) {
      const i = next[s] ?? -1;
      if (i < first || i >= last) {
        if (i >= 0) slotOf.delete(i);
        next[s] = -1;
        free.push(s);
      }
    }
    for (let i = first; i < last; i++) {
      if (slotOf.has(i)) continue;
      const s = free.length ? free.pop()! : next.length;
      next[s] = i;
      slotOf.set(i, s);
    }
    slots = next;
    queueInfo(first, last);
  }

  // Fetch ThumbHash placeholders and durations for visible tiles.
  let infoTimer = 0;
  function queueInfo(first: number, last: number) {
    clearTimeout(infoTimer);
    infoTimer = window.setTimeout(async () => {
      const ids: number[] = [];
      for (let i = first; i < last; i++) {
        const id = data.ids[i];
        if (id !== undefined && !hashes.has(id)) ids.push(id);
      }
      if (!ids.length) return;
      const infos = await api.tiles(ids.slice(0, 600));
      const h = new Map(hashes);
      const d = new Map(durations);
      for (const t of infos) {
        let url = '';
        if (t.thumbhash) {
          try {
            url = thumbHashToDataURL(Uint8Array.from(atob(t.thumbhash), (c) => c.charCodeAt(0)));
          } catch {
            url = '';
          }
        }
        h.set(t.id, url);
        if (t.duration_ms != null) d.set(t.id, t.duration_ms);
      }
      hashes = h;
      durations = d;
    }, 40);
  }

  let raf = 0;
  function onscroll() {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => {
      if (!el) return;
      scrollTop = el.scrollTop;
      refreshVisible();
    });
  }

  $effect(() => {
    void vh;
    untrack(refreshVisible);
  });

  const visibleSections = $derived.by(() => {
    if (!L || !headers) return [];
    const top = scrollTop - OVERSCAN;
    const bottom = scrollTop + vh + OVERSCAN;
    return L.sections.filter((s) => s.y >= top - 60 && s.y <= bottom);
  });

  const stickyMonth = $derived(L && headers && scrollTop > 40 ? sectionAt(L.sections, scrollTop, 'month') : null);

  function indexFromEvent(e: MouseEvent): number {
    const t = (e.target as HTMLElement).closest<HTMLElement>('[data-index]');
    return t ? Number(t.dataset.index) : -1;
  }

  function idAt(i: number) {
    return data.ids[i] ?? -1;
  }

  function rangeIds(a: number, b: number) {
    const [lo, hi] = a < b ? [a, b] : [b, a];
    return Array.from(data.ids.subarray(lo, hi + 1));
  }

  function select(i: number, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }, onCheck = false) {
    const id = idAt(i);
    if (id < 0) return;
    const add = e.ctrlKey || e.metaKey || onCheck;
    if (e.shiftKey && selection.anchor >= 0) {
      const ids = rangeIds(selection.anchor, i);
      if (add) selection.add(ids);
      else selection.set(ids);
    } else if (add) {
      selection.toggle(id);
      selection.anchor = i;
    } else {
      selection.set([id]);
      selection.anchor = i;
    }
    focus = i;
  }

  function tileRect(i: number): DOMRect | null {
    const node = el?.querySelector<HTMLElement>(`[data-index="${i}"]`);
    return node ? node.getBoundingClientRect() : null;
  }

  export function open(i = focus) {
    if (i < 0 || i >= data.count) return;
    onopen(data.ids, i, tileRect(i));
  }

  export function scrollToId(id: number) {
    const i = data.ids.indexOf(id);
    if (i < 0 || !L || !el) return;
    focus = i;
    el.scrollTop = Math.max(0, (L.y[i] ?? 0) - vh / 3);
  }

  export function ids() {
    return data.ids;
  }

  function ensureVisible(i: number) {
    if (!L || !el) return;
    const y = L.y[i] ?? 0;
    const h = L.h[i] ?? 0;
    if (y < el.scrollTop + 8) el.scrollTop = y - 60;
    else if (y + h > el.scrollTop + vh - 8) el.scrollTop = y + h - vh + 8;
  }

  /** Same column in the previous or next row. */
  function vertical(i: number, dir: 1 | -1) {
    if (!L) return i;
    const cx = (L.x[i] ?? 0) + (L.w[i] ?? 0) / 2;
    const rowY = L.y[i] ?? 0;
    let j = i;
    while (j >= 0 && j < data.count && L.y[j] === rowY) j += dir;
    if (j < 0 || j >= data.count) return i;
    const targetY = L.y[j];
    let best = j;
    let bestD = Infinity;
    while (j >= 0 && j < data.count && L.y[j] === targetY) {
      const d = Math.abs((L.x[j] ?? 0) + (L.w[j] ?? 0) / 2 - cx);
      if (d < bestD) {
        bestD = d;
        best = j;
      }
      j += dir;
    }
    return best;
  }

  function onkeydown(e: KeyboardEvent) {
    if (!data.count || viewer.ids) return;
    const mod = e.ctrlKey || e.metaKey;
    let next = focus < 0 ? 0 : focus;
    switch (e.key) {
      case 'ArrowRight':
        next = Math.min(data.count - 1, next + (focus < 0 ? 0 : 1));
        break;
      case 'ArrowLeft':
        next = Math.max(0, next - 1);
        break;
      case 'ArrowDown':
        next = focus < 0 ? 0 : vertical(next, 1);
        break;
      case 'ArrowUp':
        next = focus < 0 ? 0 : vertical(next, -1);
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = data.count - 1;
        break;
      // These open the viewer, which listens on window: stop the same
      // key event from reaching it.
      case 'Enter':
        e.preventDefault();
        e.stopPropagation();
        open(focus < 0 ? 0 : focus);
        return;
      case ' ':
        e.preventDefault();
        e.stopPropagation();
        onquicklook?.(data.ids, focus < 0 ? 0 : focus);
        return;
      case 'a':
        if (mod) {
          e.preventDefault();
          selection.set(data.ids);
        }
        return;
      case 'Escape':
        if (selection.size) {
          e.stopPropagation();
          selection.clear();
        }
        return;
      default:
        return;
    }
    e.preventDefault();
    if (e.shiftKey) {
      if (selection.anchor < 0) selection.anchor = focus < 0 ? next : focus;
      selection.set(rangeIds(selection.anchor, next));
    } else {
      selection.set([idAt(next)]);
      selection.anchor = next;
    }
    focus = next;
    ensureVisible(next);
  }

  // Pointer: click selects, double click opens, drag draws a selection band.
  let down: { x: number; y: number; i: number; ev: PointerEvent } | null = null;
  function contentPoint(e: PointerEvent) {
    const r = el!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top + el!.scrollTop };
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0 || !el) return;
    if ((e.target as HTMLElement).closest('.scrubber, .head')) return;
    el.focus({ preventScroll: true });
    const p = contentPoint(e);
    down = { ...p, i: indexFromEvent(e), ev: e };
  }

  function onpointermove(e: PointerEvent) {
    if (!down || !el || !L) return;
    const p = contentPoint(e);
    if (!band) {
      if (Math.hypot(p.x - down.x, p.y - down.y) < 6) return;
      // Capture only once dragging starts, so clicks keep their targets.
      el.setPointerCapture(e.pointerId);
    }
    band = { x0: down.x, y0: down.y, x1: p.x, y1: p.y };
    const [x0, x1] = [Math.min(band.x0, band.x1), Math.max(band.x0, band.x1)];
    const [y0, y1] = [Math.min(band.y0, band.y1), Math.max(band.y0, band.y1)];
    const hit: number[] = [];
    for (let i = firstVisible(L, y0); i < data.count && (L.y[i] ?? 0) <= y1; i++) {
      const tx = L.x[i] ?? 0;
      if (tx + (L.w[i] ?? 0) >= x0 && tx <= x1) hit.push(data.ids[i]!);
    }
    if (down.ev.ctrlKey || down.ev.metaKey) {
      selection.set(new Set([...base, ...hit]));
    } else selection.set(hit);
    // Scroll while dragging near the edges.
    const r = el.getBoundingClientRect();
    if (e.clientY > r.bottom - 30) el.scrollTop += 20;
    else if (e.clientY < r.top + 30) el.scrollTop -= 20;
  }

  let base: number[] = [];
  $effect(() => {
    if (!band) base = selection.list();
  });

  function onpointerup(e: PointerEvent) {
    if (!down) return;
    const wasBand = band !== null;
    const d = down;
    down = null;
    band = null;
    if (wasBand) return;
    const onCheck = (d.ev.target as HTMLElement).closest('.check') !== null;
    if (d.i >= 0) select(d.i, e, onCheck);
    else if (!e.ctrlKey && !e.metaKey) selection.clear();
  }

  function ondblclick(e: MouseEvent) {
    const i = indexFromEvent(e);
    if (i >= 0) open(i);
  }

  let saveTimer = 0;
  function onwheel(e: WheelEvent) {
    if (!(e.ctrlKey || e.metaKey) || !app.settings) return;
    e.preventDefault();
    const next = Math.round(Math.max(100, Math.min(420, rowHeight * (e.deltaY > 0 ? 0.9 : 1.1))));
    app.settings.row_height = next;
    clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => void app.saveSettings({ row_height: next }), 400);
  }

  function sectionIds(start: number, end: number) {
    return Array.from(data.ids.subarray(start, end));
  }
</script>

<div class="wrap">
  <div
    id="grid"
    class="scroll"
    bind:this={el}
    bind:clientWidth={width}
    bind:clientHeight={vh}
    {onscroll}
    {onkeydown}
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    {ondblclick}
    {onwheel}
    tabindex="0"
    role="grid"
    aria-label="Photos"
    aria-rowcount={-1}
    aria-multiselectable="true"
  >
    {#if L}
      <div class="content" style:height="{L.height}px">
        {#each visibleSections as s (s.kind + s.day + ':' + s.start)}
          {@const ids = sectionIds(s.start, s.end)}
          {@const all = ids.length > 0 && ids.every((id) => selection.has(id))}
          <div class="head {s.kind}" style:transform="translate({s.x}px, {s.y}px)" style:width="{s.kind === 'day' ? Math.max(s.w, 24) : s.w}px">
            <button
              class="pick"
              class:on={all}
              aria-label={all ? 'Deselect' : `Select ${s.kind === 'month' ? 'month' : 'day'}`}
              onclick={() => selection.toggleAll(ids)}><Check size={11} strokeWidth={3} /></button
            >
            {#if s.kind === 'month'}
              <h2>{monthLabel(s.day)}</h2>
            {:else}
              <h3>{s.w < 190 ? dayShort(s.day) : dayLabel(s.day)}</h3>
            {/if}
            <span class="count mono">{n(s.end - s.start)}</span>
          </div>
        {/each}
        {#each slots as idx, s (s)}
          {@const id = data.ids[idx] ?? 0}
          <Tile
            index={idx}
            hidden={idx < 0}
            url={idx >= 0 ? api.thumbUrl(id, data.versions[idx] ?? 0) : ''}
            x={L.x[idx] ?? 0}
            y={L.y[idx] ?? 0}
            w={L.w[idx] ?? 0}
            h={L.h[idx] ?? 0}
            color={colorCss(data.colors[idx] ?? 0)}
            flags={data.flags[idx] ?? 0}
            hash={hashes.get(id) || null}
            durationMs={durations.get(id) ?? null}
            selected={selection.has(id)}
            focused={idx === focus}
            label={`Item ${idx + 1} of ${data.count}`}
          />
        {/each}
        {#if band}
          <div
            class="band"
            style:left="{Math.min(band.x0, band.x1)}px"
            style:top="{Math.min(band.y0, band.y1)}px"
            style:width="{Math.abs(band.x1 - band.x0)}px"
            style:height="{Math.abs(band.y1 - band.y0)}px"
          ></div>
        {/if}
      </div>
    {:else if !loaded}
      <div class="skeleton" aria-hidden="true"></div>
    {/if}
    {#if loaded && data.count === 0}
      <div class="empty">{@render empty?.()}</div>
    {/if}
  </div>
  {#if stickyMonth}
    <div class="sticky"><span>{monthLabel(stickyMonth.day)}</span></div>
  {/if}
  {#if L && headers && L.height > vh * 2}
    <Scrubber
      sections={L.sections}
      height={L.height}
      viewport={vh}
      {scrollTop}
      onscroll={(top) => {
        if (el) el.scrollTop = top;
      }}
    />
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    height: 100%;
    min-height: 0;
  }
  .scroll {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
    scrollbar-width: none;
  }
  .scroll::-webkit-scrollbar {
    display: none;
  }
  .content {
    position: relative;
    width: 100%;
  }
  .head {
    position: absolute;
    left: 0;
    top: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    white-space: nowrap;
  }
  .head h3 {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .head.month {
    height: 52px;
    padding-top: 12px;
  }
  .head.day {
    height: 36px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  h3 {
    margin: 0;
    font-size: var(--t13);
    font-weight: 500;
  }
  .count {
    color: var(--muted);
    font-size: var(--t12);
  }
  .pick {
    display: none;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 1.5px solid var(--border-strong);
    border-radius: 50%;
    background: var(--bg);
    color: transparent;
    cursor: pointer;
  }
  .head:hover .pick,
  .pick:focus-visible,
  .pick.on {
    display: grid;
  }
  .pick.on {
    background: var(--fg);
    border-color: var(--fg);
    color: var(--bg);
  }
  .band {
    position: absolute;
    border: 1px solid var(--fg);
    background: color-mix(in srgb, var(--fg) 8%, transparent);
    pointer-events: none;
  }
  .sticky {
    position: absolute;
    top: 0;
    left: 0;
    right: 48px;
    height: 40px;
    display: flex;
    align-items: center;
    padding: 0 16px;
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(12px);
    border-bottom: 1px solid var(--border);
    font-weight: 600;
    font-size: var(--t14);
    pointer-events: none;
    z-index: 2;
  }
  .skeleton {
    position: absolute;
    inset: 88px 64px 0 16px;
    background-image: linear-gradient(var(--surface) 0 0);
    background-size: 220px 168px;
    mask-image: repeating-linear-gradient(90deg, #000 0 216px, transparent 216px 220px),
      repeating-linear-gradient(#000 0 164px, transparent 164px 168px);
    mask-composite: intersect;
  }
  .empty {
    position: absolute;
    inset: 0;
  }
</style>
