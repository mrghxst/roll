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
  import Switch from '../ui/Switch.svelte';
  import { bytes } from '../util/format';

  /** The world map's id and size (crates/core/src/offline.rs). */
  const WORLD = 'world';
  const WORLD_BYTES = 66_000_000;

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
  const online = $derived(!!app.settings?.network.online_tiles);
  const world = $derived(areas.find((a) => a.id === WORLD) ?? null);
  const regions = $derived(areas.filter((a) => a.id !== WORLD));

  async function setOnline(on: boolean) {
    if (!app.settings) return;
    try {
      await app.saveSettings({ network: { ...app.settings.network, online_tiles: on } });
    } catch (e) {
      toast.error(e);
    }
  }

  async function getWorld() {
    if (busy) return;
    busy = true;
    try {
      if (!downloadsOn && app.settings) await app.saveSettings({ network: { ...app.settings.network, downloads: true } });
      await api.addWorldMap();
      toast.show('Downloading the world map. It shows on the map when it is done; Activity has the progress.');
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

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
      toast.show(a.id === WORLD ? 'Removed the world map from this computer.' : `Removed ${a.name} from this computer.`);
    } catch (e) {
      toast.error(e);
    }
  }
</script>

<Sheet {open} title="Map detail" width={520} {onclose}>
  <section class="choice">
    <div class="row">
      <h3>Online map</h3>
      <Switch label="Use the online map" checked={online} onchange={(v) => void setOnline(v)} />
    </div>
    <p class="muted" data-testid="online-detail">
      Streets and every building everywhere, loaded from OpenFreeMap as you look. OpenFreeMap sees which areas you look at. While it is on, the map
      shows it instead of what is kept on this computer.
    </p>
  </section>
  <section class="choice">
    <div class="row">
      <h3>World map</h3>
      {#if world?.state === 'ready'}
        <span class="muted">{bytes(world.bytes)}</span>
        <button class="icon" aria-label="Remove the world map" title="Remove from this computer" onclick={() => void remove(world)}><Trash2 size={14} strokeWidth={1.5} /></button>
      {:else if world?.state === 'downloading'}
        <span class="muted">Downloading</span>
      {:else}
        <Button disabled={busy} onclick={() => void getWorld()} data-testid="get-world">
          {downloadsOn ? `Download (${bytes(WORLD_BYTES)})` : `Turn on downloads and download (${bytes(WORLD_BYTES)})`}
        </Button>
      {/if}
    </div>
    <p class="muted" data-testid="world-detail">
      Lakes, large rivers, motorways, main roads and cities for the whole world, kept on this computer (OpenStreetMap data via Protomaps).
      {#if world?.state === 'failed'}{world.error}{/if}
    </p>
  </section>
  <section class="choice">
    <h3>Offline map areas</h3>
    <p class="muted">Streets, buildings, water and place names for an area, kept on this computer, for the places that matter to you.</p>
    {#if regions.length}
      <ul class="areas" data-testid="map-areas">
        {#each regions as a (a.id)}
          <li>
            <span class="name">{a.name}</span>
            <span class="muted">
              {#if a.state === 'ready'}{bytes(a.bytes)} · {a.max_zoom >= 15 ? 'every building' : 'streets'}{:else if a.state === 'downloading'}Downloading, about {bytes(
                  a.bytes,
                )}{:else}{a.error ?? 'Stopped'}{/if}
            </span>
            <button class="icon" aria-label="Remove {a.name}" title="Remove from this computer" onclick={() => void remove(a)}><Trash2 size={14} strokeWidth={1.5} /></button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
  <h4>The area on screen</h4>
  {#if estimate?.too_big}
    <p class="warn" data-testid="area-too-big">This area is too big to keep: up to about twice Switzerland fits. Zoom in and try again.</p>
  {:else if estimate}
    <label class="field">
      <span>Name</span>
      <Input bind:value={name} aria-label="Area name" data-testid="area-name" />
    </label>
    <p class="muted" data-testid="area-detail">
      {#if estimate.max_zoom >= 15}
        Every building and footpath, about {bytes(estimate.bytes)}.
      {:else}
        Streets and larger buildings, about {bytes(estimate.bytes)}. For every building, zoom in to a city or smaller.
      {/if}
      From build.protomaps.com, which sees which area you download.
    </p>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={onclose}>Done</Button>
    <Button variant="primary" disabled={!estimate || estimate.too_big || busy} onclick={() => void keep()} data-testid="keep-area">
      {downloadsOn ? 'Keep this area offline' : 'Turn on downloads and keep this area'}
    </Button>
  {/snippet}
</Sheet>

<style>
  .choice {
    display: grid;
    gap: 6px;
    padding: 0 0 14px;
    margin: 0 0 14px;
    border-bottom: 1px solid var(--border);
  }
  .choice:last-of-type {
    padding: 0;
    margin: 0;
    border: 0;
  }
  .choice p {
    margin: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
  }
  .row h3 {
    flex: 1;
  }
  .row .muted {
    flex: none;
  }
  h3 {
    margin: 0;
    font-size: var(--t13);
    font-weight: 600;
  }
  h4 {
    margin: 14px 0 8px;
    font-size: var(--t13);
    font-weight: 500;
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
