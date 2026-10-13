<script lang="ts">
  import { Check, FileUp, MapPin, Trash2, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { PlaceGroup } from '../ipc/bindings/PlaceGroup';
  import type { PlacesInfo } from '../ipc/bindings/PlacesInfo';
  import type { TrackInfo } from '../ipc/bindings/TrackInfo';
  import { app } from '../state/app.svelte';
  import { acceptGroup, radius } from '../state/places';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import { bytes, n } from '../util/format';

  let groups = $state<PlaceGroup[]>([]);
  let tracks = $state<TrackInfo[]>([]);
  let info = $state<PlacesInfo | null>(null);
  let loading = $state(true);
  let busy = $state<string | null>(null);

  $effect(() => {
    void app.revision;
    loading = true;
    void Promise.all([api.placeGroups(), api.tracks(), api.placesInfo()])
      .then(([g, t, i]) => {
        groups = g;
        tracks = t;
        info = i;
      })
      .catch((e) => toast.error(e))
      .finally(() => (loading = false));
  });

  const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

  /** "3–10 Aug 2019", "30 Jul – 2 Aug 2019", "4 Aug 2019". */
  function span(a: string, b: string) {
    if (!a) return 'no date';
    const [ya, ma, da] = [Number(a.slice(0, 4)), Number(a.slice(5, 7)), Number(a.slice(8, 10))];
    const [yb, mb, db] = [Number(b.slice(0, 4)), Number(b.slice(5, 7)), Number(b.slice(8, 10))];
    if (a.slice(0, 10) === b.slice(0, 10)) return `${da} ${MONTHS[ma - 1]} ${ya}`;
    if (ya === yb && ma === mb) return `${da}–${db} ${MONTHS[ma - 1]} ${ya}`;
    if (ya === yb) return `${da} ${MONTHS[ma - 1]} – ${db} ${MONTHS[mb - 1]} ${ya}`;
    return `${da} ${MONTHS[ma - 1]} ${ya} – ${db} ${MONTHS[mb - 1]} ${yb}`;
  }

  const SOURCES: Record<string, string> = { track: 'From location history', neighbors: 'From your photos', event: 'From the event', name: 'From the folder name' };

  async function accept(g: PlaceGroup) {
    busy = g.key;
    await acceptGroup(g.key);
    busy = null;
  }

  async function reject(g: PlaceGroup) {
    try {
      await api.dismiss('place', g.key);
      groups = groups.filter((x) => x.key !== g.key);
    } catch (e) {
      toast.error(e);
    }
  }

  async function importTracks() {
    try {
      const t = await api.importTracks();
      if (t.length) toast.show(`Imported ${t.map((x) => `${x.name} (${n(x.count)} points)`).join(', ')}`);
    } catch (e) {
      toast.error(e);
    }
  }

  async function installPlaces() {
    try {
      const i = await api.installPlaces();
      if (i) toast.show(`Installed ${n(i.count)} place names`);
    } catch (e) {
      toast.error(e);
    }
  }
</script>

<div class="places">
  <header>
    <h1>Places</h1>
    <p class="lead">
      {n(app.counts.no_location)} items have no location. Roll suggests places from location history you import, from your own geotagged photos taken
      at the same time, and from place names in folder names. Nothing changes until you accept a suggestion.
    </p>
    <div class="row">
      <Button onclick={importTracks} data-testid="import-tracks"><FileUp size={16} strokeWidth={1.5} /> Import location history…</Button>
      <span class="hint">GPX, KML, GeoJSON, Google Maps Timeline and Google Takeout files</span>
    </div>
    {#if tracks.length}
      <ul class="tracks" aria-label="Imported location history">
        {#each tracks as t (t.id)}
          <li>
            <span class="mono">{t.name}</span>
            <span class="muted">{t.format} · {n(t.count)} points{t.start_utc ? ` · ${span(new Date(t.start_utc).toISOString(), new Date(t.end_utc ?? t.start_utc).toISOString())}` : ''}</span>
            <IconButton label="Remove {t.name}" size={28} onclick={() => void api.deleteTrack(t.id)}><Trash2 size={14} strokeWidth={1.5} /></IconButton>
          </li>
        {/each}
      </ul>
    {/if}
    {#if info && !info.installed}
      <div class="notice" role="note">
        <MapPin size={16} strokeWidth={1.5} />
        <span>
          Place names are not installed, so places show as coordinates and folder names are not read. Download them from GeoNames
          ({bytes(app.pack('places')?.size ?? 0)}), or choose a folder with <span class="mono">cities500.txt</span>,
          <span class="mono">admin1CodesASCII.txt</span> and <span class="mono">countryInfo.txt</span>.
        </span>
        {#if app.pack('places')?.state === 'downloading' || app.pack('places')?.state === 'installing'}
          <span class="muted mono">{app.pack('places')?.state === 'installing' ? 'Installing…' : `${bytes(app.pack('places')?.done ?? 0)} downloaded`}</span>
        {:else}
          <Button variant="primary" onclick={() => (app.packAsk = 'places')} data-testid="download-places">Download…</Button>
          <Button variant="ghost" onclick={installPlaces}>Choose folder…</Button>
        {/if}
      </div>
    {/if}
  </header>

  {#if loading && !groups.length}
    <p class="muted">Looking for suggestions…</p>
  {:else if !groups.length}
    <EmptyState text="No suggestions right now. Import location history, or select items and press L to set a place by hand." />
  {:else}
    <ul class="groups" data-testid="place-groups">
      {#each groups as g (g.key)}
        <li>
          <div class="text">
            <strong>{n(g.count)} {g.count === 1 ? 'item' : 'items'}, {span(g.from_local, g.to_local)}: {g.place}</strong>
            <span class="muted">{SOURCES[g.source] ?? g.source} · {g.evidence} · {radius(g.accuracy_m)}</span>
          </div>
          <div class="samples">
            {#each g.ids.slice(0, 10) as id (id)}
              <img src={api.thumbUrl(id, 1)} alt="" loading="lazy" />
            {/each}
            {#if g.count > 10}<span class="more mono">+{n(g.count - 10)}</span>{/if}
          </div>
          <div class="actions">
            <Button variant="primary" disabled={busy === g.key} onclick={() => void accept(g)} data-testid="accept-place"><Check size={14} strokeWidth={2} /> Accept</Button>
            <Button onclick={() => (app.locating = g.ids)}><MapPin size={14} strokeWidth={1.5} /> Set location…</Button>
            <Button variant="ghost" onclick={() => void reject(g)}><X size={14} strokeWidth={1.5} /> Reject</Button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .places {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  header,
  .groups {
    max-width: 920px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 16px;
    color: var(--muted);
    max-width: 72ch;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }
  .hint,
  .muted {
    color: var(--muted);
    font-size: var(--t12);
  }
  .tracks {
    margin: 0 0 16px;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  .tracks li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 6px 4px 12px;
    border-bottom: 1px solid var(--border);
    font-size: var(--t13);
  }
  .tracks li:last-child {
    border-bottom: 0;
  }
  .tracks .muted {
    flex: 1;
  }
  .notice {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 20px;
    padding: 10px 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .notice span {
    flex: 1;
  }
  .groups {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 12px;
  }
  .groups li {
    display: grid;
    gap: 10px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
  }
  .text {
    display: grid;
    gap: 2px;
    font-size: var(--t14);
  }
  .text strong {
    font-weight: 500;
  }
  .samples {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .samples img {
    width: 56px;
    height: 56px;
    object-fit: cover;
    border-radius: var(--r-1);
    background: var(--surface);
  }
  .more {
    margin-left: 6px;
    color: var(--muted);
    font-size: var(--t12);
  }
  .actions {
    display: flex;
    gap: 8px;
  }
</style>
