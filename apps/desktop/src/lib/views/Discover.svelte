<script lang="ts">
  import { ChevronDown, FolderInput, Plus, Split, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { Cluster } from '../ipc/bindings/Cluster';
  import { app } from '../state/app.svelte';
  import { setCategory } from '../state/edits';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import Input from '../ui/Input.svelte';
  import Menu, { type MenuItem } from '../ui/Menu.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { n } from '../util/format';

  let clusters = $state<Cluster[]>([]);
  let loaded = $state(false);
  let parts = $state<Record<string, Cluster[]>>({});
  let creating = $state<{ ids: number[]; name: string; definition: string; parent: string } | null>(null);

  $effect(() => {
    void app.revision;
    void api.discover(null).then((c) => {
      clusters = c;
      loaded = true;
    });
  });

  function categoryItems(ids: number[]): MenuItem[] {
    const out: MenuItem[] = [];
    for (const c of app.categories.filter((x) => x.parent_id === null && x.key !== 'unsorted')) {
      out.push({ label: c.name, run: () => void setCategory(ids, c.key) });
      for (const s of app.categories.filter((x) => x.parent_id === c.id)) out.push({ label: s.name, depth: 1, run: () => void setCategory(ids, s.key) });
    }
    out.push({ label: 'New category…', run: () => (creating = { ids, name: '', definition: '', parent: '' }) });
    return out;
  }

  async function split(c: Cluster) {
    if (parts[c.key]) {
      const { [c.key]: _, ...rest } = parts;
      parts = rest;
      return;
    }
    parts = { ...parts, [c.key]: await api.discover(c.key) };
  }

  async function ignore(c: Cluster) {
    await api.discoverIgnore(c.key);
    clusters = clusters.filter((x) => x.key !== c.key);
  }

  async function create() {
    const cr = creating;
    if (!cr || !cr.name.trim()) return;
    try {
      const key = await api.addCategory(cr.name, cr.parent || null, cr.definition);
      await app.refresh();
      creating = null;
      await setCategory(cr.ids, key);
    } catch (e) {
      toast.error(e);
    }
  }

  const name = (key: string | null) => app.categories.find((c) => c.key === key)?.name;
</script>

{#snippet card(c: Cluster, nested: boolean)}
  <li class:nested>
    <div class="head">
      <strong>{n(c.count)} items</strong>
      <span class="muted">{c.label}{c.proposed && name(c.proposed) ? ` · most are proposed as ${name(c.proposed)}` : ''}</span>
    </div>
    <div class="samples">
      {#each c.samples as id, i (id)}
        <button class="s" ondblclick={() => viewer.open(Uint32Array.from(c.ids), i, null)} aria-label="Open"><img src={api.thumbUrl(id, 1)} alt="" loading="lazy" /></button>
      {/each}
    </div>
    <div class="actions">
      <Menu label="Assign a category" items={categoryItems(c.ids)} align="left">
        {#snippet trigger({ open, toggle })}
          <Button variant="primary" onclick={toggle} aria-expanded={open} data-testid="assign-cluster"><FolderInput size={14} strokeWidth={1.5} /> Assign all to… <ChevronDown size={14} strokeWidth={1.5} /></Button>
        {/snippet}
      </Menu>
      {#if !nested}
        <Button onclick={() => void split(c)}><Split size={14} strokeWidth={1.5} /> {parts[c.key] ? 'Join again' : 'Split by folder'}</Button>
        <Button variant="ghost" onclick={() => void ignore(c)}><X size={14} strokeWidth={1.5} /> Ignore</Button>
      {/if}
    </div>
    {#if parts[c.key]}
      <ul class="parts">
        {#each parts[c.key]! as p (p.key)}{@render card(p, true)}{/each}
      </ul>
    {/if}
  </li>
{/snippet}

<div class="discover">
  <header>
    <h1>Discover</h1>
    <p class="lead">
      Undecided items that look like they belong together: same size, same encoder and no camera data usually means one app or one source,
      and with the image model, pictures that look alike. One decision sorts the whole group.
    </p>
  </header>
  {#if loaded && !clusters.length}
    <EmptyState text="No groups right now. Groups appear when five or more undecided items share a source." />
  {:else}
    <ul class="clusters" data-testid="clusters">
      {#each clusters as c (c.key)}{@render card(c, false)}{/each}
    </ul>
  {/if}
</div>

<Sheet open={creating != null} title="New category" width={440} onclose={() => (creating = null)}>
  {#if creating}
    <div class="form">
      <label><span>Name</span><Input bind:value={creating.name} placeholder="Pokémon GO" aria-label="Name" /></label>
      <label>
        <span>Inside</span>
        <select bind:value={creating.parent} aria-label="Inside">
          <option value="">Its own category</option>
          {#each app.categories.filter((c) => c.parent_id === null && c.key !== 'unsorted') as c (c.id)}<option value={c.key}>{c.name}</option>{/each}
        </select>
      </label>
      <label><span>What belongs here</span><Input bind:value={creating.definition} placeholder="Screenshots of the game" aria-label="What belongs here" /></label>
    </div>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (creating = null)}>Cancel</Button>
    <Button variant="primary" disabled={!creating?.name.trim()} onclick={create}><Plus size={14} strokeWidth={1.5} /> Create and assign {n(creating?.ids.length ?? 0)}</Button>
  {/snippet}
</Sheet>

<style>
  .discover {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  header,
  .clusters {
    max-width: 980px;
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
  .clusters,
  .parts {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 12px;
  }
  .parts {
    margin-top: 10px;
  }
  li {
    display: grid;
    gap: 10px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
  }
  li.nested {
    padding: 10px;
    background: var(--surface);
  }
  .head {
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: var(--t14);
  }
  .head strong {
    font-weight: 500;
  }
  .muted {
    color: var(--muted);
    font-size: var(--t12);
  }
  .samples {
    display: grid;
    grid-template-columns: repeat(12, 1fr);
    gap: 4px;
  }
  .s {
    padding: 0;
    border: 0;
    background: var(--surface);
    border-radius: var(--r-1);
    aspect-ratio: 1;
    cursor: pointer;
  }
  .s img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: var(--r-1);
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .form {
    display: grid;
    gap: 12px;
  }
  label {
    display: grid;
    gap: 4px;
    font-size: var(--t12);
    color: var(--muted);
  }
  select {
    height: 32px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
  }
</style>
