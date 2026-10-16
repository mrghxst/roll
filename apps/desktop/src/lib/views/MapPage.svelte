<script lang="ts">
  import { Flame, HardDriveDownload, ListOrdered, Maximize, X } from '@lucide/svelte';
  import Grid from '../grid/Grid.svelte';
  import { api } from '../ipc/api';
  import { emptyMapPoints, type MapPoints } from '../ipc/mappoints';
  import MapView from '../map/MapView.svelte';
  import OfflineSheet from '../map/OfflineSheet.svelte';
  import TimeSlider, { ymdOf } from '../map/TimeSlider.svelte';
  import Tray from '../map/Tray.svelte';
  import { app, mergeQuery } from '../state/app.svelte';
  import { setPlace } from '../state/places';
  import { selection } from '../state/selection.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import { n } from '../util/format';
  import { dayIndex } from '../map/TimeSlider.svelte';

  let points = $state<MapPoints>(emptyMapPoints());
  let days = $state<Uint32Array>(new Uint32Array(0));
  let range = $state<{ from: number; to: number } | null>(null);
  let heatmap = $state(false);
  let side = $state<{ title: string; ids: number[] } | null>(null);
  let tray = $state<{ ids: number[]; total: number }>({ ids: [], total: 0 });
  let fitKey = $state('');
  let mapView: ReturnType<typeof MapView> | undefined = $state();
  /** The offline areas sheet, with the area on screen when it opened. */
  let offline = $state<{ view: { bbox: [number, number, number, number]; lat: number; lon: number } | null } | null>(null);

  $effect(() => {
    void app.revision;
    const q = app.query;
    void Promise.all([api.mapPoints(q), api.timeline(q)]).then(([p, t]) => {
      points = p;
      days = t.days;
      if (!range && t.count) {
        let lo = Infinity;
        let hi = -Infinity;
        for (const d of t.days) {
          if (!d) continue;
          lo = Math.min(lo, d);
          hi = Math.max(hi, d);
        }
        if (lo !== Infinity) range = { from: dayIndex(lo), to: dayIndex(hi) };
        fitKey = String(Date.now());
      }
    });
  });

  // Points inside the time window.
  const visible = $derived.by(() => {
    if (!range) return null;
    const lo = ymdOf(range.from);
    const hi = ymdOf(range.to);
    const out: number[] = [];
    for (let i = 0; i < points.count; i++) {
      const d = points.days[i]!;
      if (d >= lo && d <= hi) out.push(i);
    }
    return Uint32Array.from(out);
  });

  // Items without a place in the window, for the tray (debounced).
  let trayTimer = 0;
  $effect(() => {
    const r = range;
    void app.revision;
    const q = app.query;
    clearTimeout(trayTimer);
    if (!r) return;
    trayTimer = window.setTimeout(async () => {
      const ids = await api.queryIds(mergeQuery(q, { location: 'none', from_utc: r.from * 86_400_000 - 14 * 3_600_000, to_utc: (r.to + 1) * 86_400_000 + 14 * 3_600_000 }));
      tray = { ids: ids.slice(0, 300), total: ids.length };
    }, 200);
  });

  // Selection drawn on the map shows in the side grid.
  const sideQuery = $derived(side ? mergeQuery({ ids: side.ids }) : null);

  function showInTimeline(ids: number[]) {
    selection.set(ids);
    app.go('timeline', { key: 'map-selection', label: 'From the map', query: { ids } });
  }
</script>

<div class="page">
  <div class="center">
    <div class="mapwrap">
      <MapView
        bind:this={mapView}
        {points}
        {visible}
        {heatmap}
        {fitKey}
        onopen={(ids) => (side = { title: ids.length === 1 ? '1 item' : `${n(ids.length)} items`, ids })}
        onselect={(ids) => {
          selection.set(ids);
          side = ids.length ? { title: `${n(ids.length)} selected`, ids } : null;
        }}
        ondropids={(ids, lat, lon, accuracy_m) => void setPlace(ids, { lat, lon, accuracy_m, name: null })}
      />
      <div class="tools">
        <span class="count mono">{n(visible?.length ?? points.count)} on the map</span>
        <IconButton label="Show everything in the time range" onclick={() => mapView?.fitAll()} disabled={!points.count}><Maximize size={16} strokeWidth={1.5} /></IconButton>
        <IconButton label={heatmap ? 'Show clusters' : 'Show a heatmap'} active={heatmap} onclick={() => (heatmap = !heatmap)}><Flame size={16} strokeWidth={1.5} /></IconButton>
        <IconButton label="Offline map areas" onclick={() => (offline = { view: mapView?.view() ?? null })} data-testid="offline-areas"><HardDriveDownload size={16} strokeWidth={1.5} /></IconButton>
      </div>
      <p class="hint">Shift+drag selects a rectangle, Alt+drag draws a lasso.</p>
      {#if range}
        <TimeSlider {days} from={range.from} to={range.to} onchange={(from, to) => (range = { from, to })} />
      {/if}
    </div>
    <Tray ids={tray.ids} total={tray.total} />
  </div>
  <OfflineSheet open={offline != null} view={offline?.view ?? null} onclose={() => (offline = null)} />
  {#if side && sideQuery}
    <aside class="side" aria-label="Items on the map">
      <header>
        <span>{side.title}</span>
        <Button variant="ghost" onclick={() => side && showInTimeline(side.ids)}><ListOrdered size={14} strokeWidth={1.5} /> Show in timeline</Button>
        <IconButton label="Close" size={28} onclick={() => (side = null)}><X size={16} strokeWidth={1.5} /></IconButton>
      </header>
      <div class="grid">
        <Grid query={sideQuery} headers={false} onopen={(ids, i, rect) => viewer.open(ids, i, rect)} onquicklook={(ids, i) => viewer.open(ids, i, null, true)} />
      </div>
    </aside>
  {/if}
</div>

<style>
  .page {
    display: flex;
    height: 100%;
  }
  .center {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .mapwrap {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .tools {
    position: absolute;
    top: 10px;
    left: 10px;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 3px 3px 10px;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(12px);
    box-shadow: 0 4px 18px rgba(0, 0, 0, 0.08);
    font-size: var(--t12);
  }
  .hint {
    position: absolute;
    top: 18px;
    left: 50%;
    z-index: 1;
    margin: 0;
    padding: 3px 10px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg) 80%, transparent);
    color: var(--muted);
    font-size: 11px;
    transform: translateX(-50%);
    pointer-events: none;
  }
  .side {
    display: flex;
    flex-direction: column;
    width: 380px;
    border-left: 1px solid var(--border);
    background: var(--bg);
  }
  .side header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px 6px 14px;
    border-bottom: 1px solid var(--border);
    font-size: var(--t13);
  }
  .side header span {
    flex: 1;
  }
  .grid {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
