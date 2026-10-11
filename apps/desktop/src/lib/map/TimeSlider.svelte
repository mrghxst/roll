<script lang="ts" module>
  /** yyyymmdd → days since 1970-01-01. */
  export function dayIndex(ymd: number): number {
    const y = Math.floor(ymd / 10000);
    const m = Math.floor((ymd / 100) % 100);
    const d = ymd % 100;
    return Math.floor(Date.UTC(y, Math.max(0, m - 1), Math.max(1, d)) / 86_400_000);
  }

  export function ymdOf(day: number): number {
    const d = new Date(day * 86_400_000);
    return d.getUTCFullYear() * 10000 + (d.getUTCMonth() + 1) * 100 + d.getUTCDate();
  }
</script>

<script lang="ts">
  import { Pause, Play } from '@lucide/svelte';
  import { onDestroy } from 'svelte';

  type Props = {
    /** yyyymmdd of every item in scope (with or without a place). */
    days: Uint32Array;
    /** Selected range in day indices, inclusive. */
    from: number;
    to: number;
    onchange: (from: number, to: number) => void;
  };

  let { days, from, to, onchange }: Props = $props();

  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

  let track: HTMLDivElement | undefined = $state();
  let height = $state(400);
  let playing = $state(false);
  let speed = $state(1);
  let timer = 0;

  // The whole library, padded to whole months.
  const domain = $derived.by(() => {
    let lo = Infinity;
    let hi = -Infinity;
    for (const d of days) {
      if (!d) continue;
      lo = Math.min(lo, d);
      hi = Math.max(hi, d);
    }
    if (lo === Infinity) {
      const now = Math.floor(Date.now() / 86_400_000);
      return { lo: now - 365, hi: now };
    }
    const a = new Date(dayIndex(lo) * 86_400_000);
    const b = new Date(dayIndex(hi) * 86_400_000);
    return {
      lo: Math.floor(Date.UTC(a.getUTCFullYear(), a.getUTCMonth(), 1) / 86_400_000),
      hi: Math.floor(Date.UTC(b.getUTCFullYear(), b.getUTCMonth() + 1, 1) / 86_400_000) - 1,
    };
  });

  let view = $state<{ lo: number; hi: number } | null>(null);
  const v = $derived(view ?? domain);

  // Week bins when less than about a year and a half is in view.
  const weekly = $derived(v.hi - v.lo < 540);

  function binStart(day: number): number {
    if (weekly) return day - ((day + 3) % 7); // Mondays
    const d = new Date(day * 86_400_000);
    return Math.floor(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1) / 86_400_000);
  }

  function nextBin(start: number): number {
    if (weekly) return start + 7;
    const d = new Date(start * 86_400_000);
    return Math.floor(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1) / 86_400_000);
  }

  const counts = $derived.by(() => {
    const m = new Map<number, number>();
    for (const d of days) {
      if (!d) continue;
      const di = dayIndex(d);
      if (di < v.lo || di > v.hi) continue;
      const b = binStart(di);
      m.set(b, (m.get(b) ?? 0) + 1);
    }
    return m;
  });

  const bins = $derived.by(() => {
    const out: { start: number; end: number; n: number }[] = [];
    for (let s = binStart(v.lo); s <= v.hi; s = nextBin(s)) out.push({ start: s, end: nextBin(s) - 1, n: counts.get(s) ?? 0 });
    return out;
  });
  const max = $derived(Math.max(1, ...bins.map((b) => b.n)));

  const years = $derived.by(() => {
    const out: number[] = [];
    const a = new Date(domain.lo * 86_400_000).getUTCFullYear();
    const b = new Date(domain.hi * 86_400_000).getUTCFullYear();
    for (let y = b; y >= a; y--) out.push(y);
    return out;
  });

  // Newest at the top.
  const yOf = (day: number) => ((v.hi + 1 - day) / (v.hi + 1 - v.lo)) * height;
  const dayOf = (y: number) => Math.round(v.hi + 1 - (y / height) * (v.hi + 1 - v.lo));

  function label(day: number) {
    const d = new Date(day * 86_400_000);
    return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
  }

  let drag: { mode: 'from' | 'to' | 'move' | 'new'; y: number; from: number; to: number } | null = null;

  function clamp(a: number, b: number): [number, number] {
    const lo = Math.max(domain.lo, Math.min(a, b));
    const hi = Math.min(domain.hi, Math.max(a, b));
    return [lo, hi];
  }

  function down(e: PointerEvent, mode: 'from' | 'to' | 'move' | 'new') {
    e.stopPropagation();
    e.preventDefault();
    const y = e.clientY - track!.getBoundingClientRect().top;
    drag = { mode, y, from, to };
    if (mode === 'new') {
      const d = dayOf(y);
      onchange(d, d);
      drag = { mode: 'from', y, from: d, to: d };
    }
    track!.setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    const y = e.clientY - track!.getBoundingClientRect().top;
    const d = dayOf(y);
    if (drag.mode === 'from') onchange(...clamp(d, drag.to));
    else if (drag.mode === 'to') onchange(...clamp(drag.from, d));
    else {
      const delta = d - dayOf(drag.y);
      const span = drag.to - drag.from;
      let a = drag.from + delta;
      a = Math.max(domain.lo, Math.min(domain.hi - span, a));
      onchange(a, a + span);
    }
  }

  function wheel(e: WheelEvent) {
    e.preventDefault();
    const y = e.clientY - track!.getBoundingClientRect().top;
    const at = dayOf(y);
    const f = e.deltaY > 0 ? 1.25 : 0.8;
    const lo = Math.max(domain.lo, Math.round(at - (at - v.lo) * f));
    const hi = Math.min(domain.hi, Math.round(at + (v.hi - at) * f));
    view = hi - lo < 28 ? view : { lo, hi };
  }

  function pickYear(y: number) {
    const a = Math.floor(Date.UTC(y, 0, 1) / 86_400_000);
    const b = Math.floor(Date.UTC(y + 1, 0, 1) / 86_400_000) - 1;
    onchange(...clamp(a, b));
    view = { lo: Math.max(domain.lo, a - 120), hi: Math.min(domain.hi, b + 120) };
  }

  function togglePlay() {
    playing = !playing;
    clearInterval(timer);
    if (!playing) return;
    // Start from the oldest end when the window is at the end already.
    if (to >= domain.hi) {
      const span = to - from;
      onchange(domain.lo, domain.lo + span);
    }
    timer = window.setInterval(() => {
      const span = to - from;
      const step = Math.max(1, Math.round(span / 4));
      if (to + step > domain.hi) {
        playing = false;
        clearInterval(timer);
        onchange(domain.hi - span, domain.hi);
        return;
      }
      onchange(from + step, to + step);
    }, 600 / speed);
  }

  onDestroy(() => clearInterval(timer));
</script>

<div class="slider" aria-label="Time range">
  <div class="years" role="group" aria-label="Years">
    {#each years as y (y)}
      <button class:on={new Date(from * 86_400_000).getUTCFullYear() <= y && new Date(to * 86_400_000).getUTCFullYear() >= y} onclick={() => pickYear(y)}>{y}</button>
    {/each}
  </div>
  <div
    class="track"
    bind:this={track}
    bind:clientHeight={height}
    onwheel={wheel}
    onpointerdown={(e) => down(e, 'new')}
    onpointermove={move}
    onpointerup={() => (drag = null)}
    role="slider"
    tabindex="0"
    aria-valuemin={domain.lo}
    aria-valuemax={domain.hi}
    aria-valuenow={from}
    aria-valuetext="{label(from)} to {label(to)}"
    aria-label="Drag to choose the time range; scroll to zoom"
    onkeydown={(e) => {
      const step = weekly ? 7 : 30;
      if (e.key === 'ArrowUp') onchange(...clamp(from + step, to + step));
      else if (e.key === 'ArrowDown') onchange(...clamp(from - step, to - step));
    }}
  >
    {#each bins as b (b.start)}
      <div class="bar" style:top="{yOf(b.end + 1)}px" style:height="{Math.max(1, yOf(b.start) - yOf(b.end + 1) - 1)}px" style:width="{(b.n / max) * 100}%" class:in={b.end >= from && b.start <= to}></div>
    {/each}
    <div class="range" style:top="{yOf(to + 1)}px" style:height="{Math.max(4, yOf(from) - yOf(to + 1))}px" onpointerdown={(e) => down(e, 'move')} role="presentation">
      <div class="handle top" onpointerdown={(e) => down(e, 'to')} role="presentation"></div>
      <div class="handle bottom" onpointerdown={(e) => down(e, 'from')} role="presentation"></div>
    </div>
  </div>
  <div class="foot">
    <button class="play" onclick={togglePlay} aria-label={playing ? 'Pause' : 'Play through time'}>
      {#if playing}<Pause size={14} strokeWidth={1.5} />{:else}<Play size={14} strokeWidth={1.5} />{/if}
    </button>
    <select bind:value={speed} aria-label="Speed">
      <option value={0.5}>0.5×</option>
      <option value={1}>1×</option>
      <option value={2}>2×</option>
      <option value={4}>4×</option>
    </select>
  </div>
  <div class="dates mono" aria-live="polite">
    <span>{label(to)}</span>
    <span>{label(from)}</span>
  </div>
</div>

<style>
  .slider {
    display: grid;
    grid-template-columns: 44px 1fr;
    grid-template-rows: 1fr auto auto;
    width: 148px;
    height: 100%;
    border-right: 1px solid var(--border);
    background: var(--bg);
    user-select: none;
  }
  .years {
    grid-row: 1 / 2;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 4px;
    overflow-y: auto;
  }
  .years button {
    padding: 2px 0;
    border: 0;
    border-radius: var(--r-1);
    background: transparent;
    color: var(--muted);
    font: 500 11px var(--mono);
    cursor: pointer;
  }
  .years button:hover {
    color: var(--fg);
  }
  .years button.on {
    background: var(--fg);
    color: var(--bg);
  }
  .track {
    position: relative;
    margin: 8px 8px 8px 0;
    border-left: 1px solid var(--border);
    cursor: crosshair;
    outline: none;
    touch-action: none;
  }
  .track:focus-visible {
    box-shadow: 0 0 0 2px var(--fg);
  }
  .bar {
    position: absolute;
    left: 0;
    background: var(--border-strong);
  }
  .bar.in {
    background: var(--fg);
  }
  .range {
    position: absolute;
    left: -4px;
    right: 0;
    border: 1px solid var(--fg);
    border-radius: 3px;
    background: color-mix(in srgb, var(--fg) 6%, transparent);
    cursor: grab;
  }
  .handle {
    position: absolute;
    left: 0;
    right: 0;
    height: 8px;
    cursor: ns-resize;
  }
  .handle.top {
    top: -4px;
  }
  .handle.bottom {
    bottom: -4px;
  }
  .foot {
    grid-column: 1 / 3;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-top: 1px solid var(--border);
  }
  .play {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    cursor: pointer;
  }
  select {
    height: 28px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t12);
  }
  .dates {
    grid-column: 1 / 3;
    display: grid;
    gap: 2px;
    padding: 6px 8px 8px;
    color: var(--muted);
    font-size: 11px;
  }
</style>
