<script lang="ts">
  import { ClipboardPaste, MapPin } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { PlaceHit } from '../ipc/bindings/PlaceHit';
  import { app } from '../state/app.svelte';
  import { clearPlace, radius, setPlace } from '../state/places';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Sheet from '../ui/Sheet.svelte';

  let text = $state('');
  let hits = $state<PlaceHit[]>([]);
  let active = $state(0);
  let el: HTMLInputElement | undefined = $state();
  let timer = 0;
  let seq = 0;

  const ids = $derived(app.locating);

  $effect(() => {
    if (ids) {
      text = '';
      hits = [];
      active = 0;
      queueMicrotask(() => el?.focus());
    }
  });

  function search() {
    clearTimeout(timer);
    const q = text;
    const my = ++seq;
    timer = window.setTimeout(async () => {
      const r = q.trim() ? await api.placeSearch(q) : [];
      if (my === seq) {
        hits = r;
        active = 0;
      }
    }, 120);
  }

  async function choose(h: PlaceHit | undefined) {
    const target = ids;
    if (!h || !target) return;
    app.locating = null;
    await setPlace(target, { lat: h.lat, lon: h.lon, accuracy_m: h.accuracy_m, name: h.label });
  }

  async function clear() {
    const target = ids;
    if (!target) return;
    app.locating = null;
    await clearPlace(target);
  }

  const KIND: Record<string, string> = { city: 'Place', region: 'Region', country: 'Country' };
</script>

<Sheet open={ids != null} title="Set location" width={480} onclose={() => (app.locating = null)}>
  <p class="lead">For {ids?.length === 1 ? 'this item' : `${ids?.length ?? 0} items`}. Search a place, or paste coordinates or a link from Google Maps, Apple Maps or OpenStreetMap.</p>
  <Input
    bind:el
    bind:value={text}
    oninput={search}
    placeholder="Lake Garda, 45.6789, 10.6543, or a map link"
    aria-label="Place or coordinates"
    data-testid="location-input"
    onkeydown={(e) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        active = Math.min(hits.length - 1, active + 1);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        active = Math.max(0, active - 1);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        void choose(hits[active]);
      }
    }}
  />
  {#if hits.length}
    <ul class="hits" role="listbox" aria-label="Places">
      {#each hits as h, i (h.label + h.lat)}
        <li role="option" aria-selected={i === active}>
          <button class:active={i === active} onclick={() => void choose(h)} onpointermove={() => (active = i)}>
            <MapPin size={14} strokeWidth={1.5} />
            <span class="label">{h.label}</span>
            <span class="meta">{h.name.match(/^-?\d/) ? 'Coordinates' : KIND[h.kind]} · {radius(h.accuracy_m)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {:else if text.trim()}
    <p class="muted">No match. Paste coordinates to set an exact point.</p>
  {/if}
  {#if app.copiedPlace}
    {@const c = app.copiedPlace}
    <button class="copied" onclick={() => void choose({ label: c.label, name: c.label, kind: 'city', lat: c.lat, lon: c.lon, accuracy_m: c.accuracy_m })}>
      <ClipboardPaste size={14} strokeWidth={1.5} /> Use the copied place: {c.label}
    </button>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={clear}>Clear location</Button>
    <span style:flex="1"></span>
    <Button variant="ghost" onclick={() => (app.locating = null)}>Cancel</Button>
  {/snippet}
</Sheet>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: var(--t13);
  }
  .muted {
    color: var(--muted);
    font-size: var(--t13);
  }
  .hits {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .hits button,
  .copied {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--fg);
    font-size: var(--t13);
    text-align: left;
    cursor: pointer;
  }
  .hits button.active,
  .copied:hover {
    background: var(--surface);
  }
  .label {
    flex: 1;
  }
  .meta {
    color: var(--muted);
    font-size: var(--t12);
  }
  .copied {
    margin-top: 10px;
    border: 1px dashed var(--border-strong);
  }
</style>
