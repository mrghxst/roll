<script lang="ts">
  import { api } from '../ipc/api';
  import type { Facet } from '../ipc/bindings/Facet';
  import type { Facets } from '../ipc/bindings/Facets';
  import type { Query } from '../ipc/bindings/Query';
  import { app, mergeQuery } from '../state/app.svelte';
  import { compact } from '../util/format';

  let facets = $state<Facets | null>(null);
  let seq = 0;
  $effect(() => {
    const q = mergeQuery(app.scope.query, app.filters);
    void app.revision;
    const my = ++seq;
    const t = setTimeout(() => {
      void api.facets(q).then((f) => {
        if (my === seq) facets = f;
      });
    }, 120);
    return () => clearTimeout(t);
  });

  type ListKey = 'media' | 'categories' | 'cameras' | 'lenses' | 'formats' | 'tags';

  function toggleList(key: ListKey, value: string) {
    const cur = (app.filters[key] as string[] | undefined) ?? [];
    const next = cur.includes(value) ? cur.filter((v) => v !== value) : [...cur, value];
    app.filters = { ...app.filters, [key]: next };
  }

  function setOne<K extends keyof Query>(key: K, value: Query[K]) {
    app.filters = { ...app.filters, [key]: app.filters[key] === value ? null : value };
  }

  function year(y: string) {
    const from = Date.UTC(Number(y), 0, 1);
    const to = Date.UTC(Number(y) + 1, 0, 1);
    const on = app.filters.from_utc === from && app.filters.to_utc === to;
    app.filters = { ...app.filters, from_utc: on ? null : from, to_utc: on ? null : to };
  }

  const isYear = (y: string) => app.filters.from_utc === Date.UTC(Number(y), 0, 1) && app.filters.to_utc === Date.UTC(Number(y) + 1, 0, 1);
  const PROBLEM_BITS: Record<string, number> = { no_date: 1, suspicious_date: 2, date_changed: 4, no_location: 8 };
</script>

{#snippet list(title: string, items: Facet[], active: (f: Facet) => boolean, onpick: (f: Facet) => void, limit = 8)}
  {#if items.length}
    <section>
      <h4>{title}</h4>
      {#each items.slice(0, limit) as f (f.key)}
        <button class="row" class:on={active(f)} onclick={() => onpick(f)} aria-pressed={active(f)}>
          <span class="label">{f.label}</span>
          <span class="mono count">{compact(f.count)}</span>
        </button>
      {/each}
    </section>
  {/if}
{/snippet}

<aside class="filters" aria-label="Filters">
  <div class="head">
    <span>Filters</span>
    {#if Object.keys(app.filters).length}
      <button class="clear" onclick={() => (app.filters = {})}>Clear</button>
    {/if}
  </div>
  {#if facets}
    {#if facets.years.length}
      <section>
        <h4>Year</h4>
        <div class="chips">
          {#each facets.years as y (y.key)}
            <button class="chip mono" class:on={isYear(y.key)} onclick={() => year(y.key)} title="{compact(y.count)} items">{y.key}</button>
          {/each}
        </div>
      </section>
    {/if}
    {@render list('Type', facets.media, (f) => (app.filters.media ?? []).includes(f.key), (f) => toggleList('media', f.key))}
    {@render list('Category', facets.categories, (f) => (app.filters.categories ?? []).includes(f.key), (f) => toggleList('categories', f.key))}
    {@render list('Location', facets.location, (f) => app.filters.location === (f.key === 'exact' ? 'exact' : f.key), (f) => setOne('location', f.key))}
    {@render list('Camera', facets.cameras, (f) => (app.filters.cameras ?? []).includes(f.key), (f) => toggleList('cameras', f.key))}
    {@render list('Lens', facets.lenses, (f) => (app.filters.lenses ?? []).includes(f.key), (f) => toggleList('lenses', f.key), 5)}
    {@render list('Tags', facets.tags, (f) => (app.filters.tags ?? []).includes(f.key), (f) => toggleList('tags', f.key))}
    {@render list('Format', facets.formats, (f) => (app.filters.formats ?? []).includes(f.key), (f) => toggleList('formats', f.key))}
    {@render list('Orientation', facets.orientation, (f) => app.filters.orientation === f.key, (f) => setOne('orientation', f.key))}
    {@render list('Problems', facets.problems, (f) => app.filters.problems === PROBLEM_BITS[f.key], (f) => setOne('problems', PROBLEM_BITS[f.key] ?? null))}
    {@render list('Immich', facets.immich, (f) => app.filters.immich === f.key, (f) => setOne('immich', f.key))}
    <section>
      <h4>Rating</h4>
      <div class="chips">
        {#each [1, 2, 3, 4, 5] as r (r)}
          <button class="chip mono" class:on={app.filters.min_rating === r} onclick={() => setOne('min_rating', r)}>{r}+</button>
        {/each}
        <button class="chip" class:on={app.filters.favorite === true} onclick={() => setOne('favorite', true)}>Favorites</button>
      </div>
    </section>
  {/if}
</aside>

<style>
  .filters {
    width: 248px;
    flex: none;
    overflow-y: auto;
    padding: 0 12px 24px;
    border-right: 1px solid var(--border);
    font-size: var(--t13);
  }
  .head {
    position: sticky;
    top: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 44px;
    padding: 0 4px;
    background: var(--bg);
    font-weight: 600;
    z-index: 1;
  }
  .clear {
    border: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
    font-size: var(--t12);
  }
  .clear:hover {
    color: var(--fg);
  }
  section {
    padding: 8px 0;
    border-top: 1px solid var(--border);
  }
  h4 {
    margin: 4px 4px 6px;
    color: var(--muted);
    font-size: var(--t12);
    font-weight: 500;
  }
  .row {
    display: flex;
    align-items: center;
    width: 100%;
    height: 28px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--r-1);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface);
  }
  .row.on {
    background: var(--fg);
    color: var(--bg);
  }
  .row.on .count {
    color: var(--bg);
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    color: var(--muted);
    font-size: var(--t12);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 0 4px;
  }
  .chip {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    background: var(--bg);
    font-size: var(--t12);
    cursor: pointer;
  }
  .chip:hover {
    border-color: var(--border-strong);
  }
  .chip.on {
    background: var(--fg);
    border-color: var(--fg);
    color: var(--bg);
  }
</style>
