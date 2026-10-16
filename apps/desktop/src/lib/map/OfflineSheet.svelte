<script lang="ts">
  import { Trash2 } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { MapArea } from '../ipc/bindings/MapArea';
  import type { MapEstimate } from '../ipc/bindings/MapEstimate';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { bytes, n } from '../util/format';

  type View = { bbox: [number, number, number, number]; lat: number; lon: number };
  let { open, view, onclose }: { open: boolean; view: View | null; onclose: () => void } = $props();

  let areas = $state<MapArea[]>([]);
  let estimate = $state<MapEstimate | null>(null);
  let name = $state('');
  let busy = $state(false);

  $effect(() => {
    void app.revision;
    if (!open) return;
    void api.mapAreas().then((a) => (areas = a));
  });

  // A name and an estimate for the area on screen when the sheet opens.
  $effect(() => {
    const v = view;
    if (!open || !v) return;
    estimate = null;
    void api.mapEstimate(v.bbox).then((e) => (estimate = e));
    void api
      .placeName(v.lat, v.lon)
      .then((p) => (name = p ? `Around ${p.split(',')[0]}` : 'Map area'))
      .catch(() => (name = 'Map area'));
  });

  const downloadsOn = $derived(!!app.settings?.network.downloads);

  async function keep() {
    if (!view || busy) return;
    busy = true;
    try {
      if (!downloadsOn && app.settings) await app.saveSettings({ network: { ...app.settings.network, downloads: true } });
      await api.addMapArea(name.trim() || 'Map area', view.bbox);
      toast.show('Downloading the map area. It shows on the map when it is done; Activity has the progress.');
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  async function remove(a: MapArea) {
    try {
      await api.removeMapArea(a.id);
      toast.show(`Removed ${a.name} from this computer.`);
    } catch (e) {
      toast.error(e);
    }
  }
</script>

<Sheet {open} title="Offline map areas" width={520} {onclose}>
  <p class="lead">
    Streets, water and place names for an area, kept on this computer (zoom levels 0 to 13, OpenStreetMap data via Protomaps). The map shows them while
    Settings › Network › Online map tiles is off.
  </p>
  {#if areas.length}
    <ul class="areas" data-testid="map-areas">
      {#each areas as a (a.id)}
        <li>
          <span class="name">{a.name}</span>
          <span class="muted">
            {#if a.state === 'ready'}{bytes(a.bytes)} · {n(a.tiles)} tiles{:else if a.state === 'downloading'}Downloading, about {bytes(a.bytes)}{:else}{a.error ?? 'Stopped'}{/if}
          </span>
          <button class="icon" aria-label="Remove {a.name}" title="Remove from this computer" onclick={() => void remove(a)}><Trash2 size={14} strokeWidth={1.5} /></button>
        </li>
      {/each}
    </ul>
  {/if}
  <h3>The area on screen</h3>
  {#if estimate?.too_big}
    <p class="warn" data-testid="area-too-big">This area needs {n(estimate.tiles)} tiles; up to 20,000 fit (about twice Switzerland). Zoom in and try again.</p>
  {:else if estimate}
    <label class="field">
      <span>Name</span>
      <Input bind:value={name} aria-label="Area name" data-testid="area-name" />
    </label>
    <p class="muted">{n(estimate.tiles)} tiles, about {bytes(estimate.bytes)}. From build.protomaps.com, which sees which area you download.</p>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={onclose}>Done</Button>
    <Button variant="primary" disabled={!estimate || estimate.too_big || busy} onclick={() => void keep()} data-testid="keep-area">
      {downloadsOn ? 'Keep this area offline' : 'Turn on downloads and keep this area'}
    </Button>
  {/snippet}
</Sheet>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: var(--t13);
  }
  h3 {
    margin: 16px 0 8px;
    font-size: var(--t13);
    font-weight: 600;
  }
  .areas {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .areas li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .name {
    font-weight: 500;
  }
  .muted {
    flex: 1;
    color: var(--muted);
    font-size: var(--t12);
  }
  .warn {
    color: var(--danger);
    font-size: var(--t13);
  }
  .field {
    display: grid;
    gap: 4px;
    font-size: var(--t13);
  }
  .field span {
    color: var(--muted);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: var(--r-1);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--surface);
    color: var(--fg);
  }
</style>
