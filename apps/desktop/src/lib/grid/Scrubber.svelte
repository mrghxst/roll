<script lang="ts">
  import type { Section } from '../layout/justified';
  import { monthShort, splitDay } from '../util/format';

  let {
    sections,
    height,
    viewport,
    scrollTop,
    onscroll,
  }: { sections: Section[]; height: number; viewport: number; scrollTop: number; onscroll: (top: number) => void } = $props();

  let track: HTMLDivElement | undefined = $state();
  let trackH = $state(0);
  let hoverY = $state<number | null>(null);
  let dragging = $state(false);

  const months = $derived(sections.filter((s) => s.kind === 'month'));
  const scrollable = $derived(Math.max(1, height - viewport));

  /** Year labels, spaced so they never overlap. */
  const years = $derived.by(() => {
    const out: { label: string; y: number }[] = [];
    let lastYear = -1;
    let lastY = -100;
    for (const m of months) {
      const { y } = splitDay(m.day);
      if (y === lastYear) continue;
      lastYear = y;
      const py = (Math.min(m.y, scrollable) / scrollable) * trackH;
      if (py - lastY < 18) continue;
      out.push({ label: y ? String(y) : '—', y: py });
      lastY = py;
    }
    return out;
  });

  const thumbY = $derived((scrollTop / scrollable) * trackH);

  function monthAt(py: number) {
    const top = (py / Math.max(1, trackH)) * scrollable;
    let found = months[0];
    for (const m of months) {
      if (m.y <= top) found = m;
      else break;
    }
    if (!found) return '';
    const { y, m } = splitDay(found.day);
    return y ? `${monthShort(m)} ${y}` : 'No date';
  }

  let raf = 0;
  function seek(clientY: number) {
    if (!track) return;
    const r = track.getBoundingClientRect();
    const py = Math.max(0, Math.min(r.height, clientY - r.top));
    hoverY = py;
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => onscroll((py / r.height) * scrollable));
  }
</script>

<div
  class="scrubber"
  class:dragging
  bind:this={track}
  bind:clientHeight={trackH}
  role="scrollbar"
  aria-controls="grid"
  aria-orientation="vertical"
  aria-valuenow={Math.round((scrollTop / scrollable) * 100)}
  aria-valuemin={0}
  aria-valuemax={100}
  tabindex="-1"
  onpointerdown={(e) => {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    seek(e.clientY);
  }}
  onpointermove={(e) => {
    if (dragging) seek(e.clientY);
    else if (track) hoverY = e.clientY - track.getBoundingClientRect().top;
  }}
  onpointerup={() => (dragging = false)}
  onpointerleave={() => {
    if (!dragging) hoverY = null;
  }}
>
  {#each years as yr (yr.label + yr.y)}
    <span class="year mono" style:top="{yr.y}px">{yr.label}</span>
  {/each}
  <span class="thumb" style:top="{thumbY}px"></span>
  {#if hoverY !== null}
    <span class="hover mono" style:top="{hoverY}px">{monthAt(hoverY)}</span>
  {/if}
</div>

<style>
  .scrubber {
    position: absolute;
    top: 8px;
    right: 0;
    bottom: 8px;
    width: 44px;
    cursor: ns-resize;
    touch-action: none;
    user-select: none;
    z-index: 3;
  }
  .year {
    position: absolute;
    right: 8px;
    transform: translateY(-2px);
    font-size: 10px;
    color: var(--muted);
    pointer-events: none;
    opacity: 0.8;
  }
  .scrubber:hover .year,
  .dragging .year {
    opacity: 1;
  }
  .thumb {
    position: absolute;
    right: 2px;
    width: 3px;
    height: 24px;
    margin-top: -12px;
    border-radius: 2px;
    background: var(--fg);
    opacity: 0.5;
    pointer-events: none;
  }
  .hover {
    position: absolute;
    right: 40px;
    transform: translateY(-50%);
    padding: 3px 8px;
    border-radius: var(--r-1);
    background: var(--fg);
    color: var(--bg);
    font-size: var(--t12);
    white-space: nowrap;
    pointer-events: none;
  }
</style>
