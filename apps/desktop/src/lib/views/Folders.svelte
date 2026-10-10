<script lang="ts">
  import { ChevronRight, FolderOpen } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { FolderNode } from '../ipc/bindings/FolderNode';
  import Grid from '../grid/Grid.svelte';
  import { app, mergeQuery } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { viewer } from '../state/viewer.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Switch from '../ui/Switch.svelte';
  import { compact } from '../util/format';

  let roots = $state<FolderNode[]>([]);
  let current = $state<{ root: number; path: string; name: string } | null>(null);
  let open = $state(new Set<string>());
  let recursive = $state(false);

  $effect(() => {
    void app.revision;
    void api.folders().then((f) => {
      roots = f;
      if (!current && f[0]) current = { root: f[0].root_id, path: '', name: f[0].name };
      if (f[0] && open.size === 0) open = new Set([`${f[0].root_id}:`]);
    });
  });

  const key = (n: FolderNode) => `${n.root_id}:${n.path}`;

  function toggle(n: FolderNode) {
    const next = new Set(open);
    if (next.has(key(n))) next.delete(key(n));
    else next.add(key(n));
    open = next;
  }

  function pick(n: FolderNode) {
    selection.clear();
    current = { root: n.root_id, path: n.path, name: n.name };
    if (!open.has(key(n))) toggle(n);
  }

  const query = $derived(current ? mergeQuery(app.filters, { root_id: current.root, folder: current.path, recursive }) : null);
</script>

{#snippet node(n: FolderNode, depth: number)}
  <div class="node" class:active={current?.root === n.root_id && current.path === n.path} style:padding-left="{depth * 14 + 6}px">
    {#if n.children.length}
      <button class="twisty" class:open={open.has(key(n))} aria-label={open.has(key(n)) ? 'Collapse' : 'Expand'} onclick={() => toggle(n)}>
        <ChevronRight size={14} strokeWidth={1.5} />
      </button>
    {:else}
      <span class="twisty"></span>
    {/if}
    <button class="name" onclick={() => pick(n)} title={n.path || n.name}>
      <span>{n.name}</span>
      <span class="mono count">{compact(n.total)}</span>
    </button>
  </div>
  {#if open.has(key(n))}
    {#each n.children as c (key(c))}{@render node(c, depth + 1)}{/each}
  {/if}
{/snippet}

<div class="folders">
  <nav class="tree" aria-label="Folders">
    {#each roots as r (key(r))}{@render node(r, 0)}{/each}
  </nav>
  <div class="pane">
    {#if current && query}
      <div class="bar">
        <span class="path" title={current.path}>{current.path || current.name}</span>
        <label class="sub"><Switch label="Include subfolders" bind:checked={recursive} /> Include subfolders</label>
        <IconButton label="Show in Explorer" onclick={() => current && void api.openFolder(current.root, current.path)}>
          <FolderOpen size={16} strokeWidth={1.5} />
        </IconButton>
      </div>
      <div class="grid">
        <Grid
          {query}
          headers={recursive}
          onopen={(ids, index, rect) => viewer.open(ids, index, rect)}
          onquicklook={(ids, index) => viewer.open(ids, index, null, true)}
        >
          {#snippet empty()}
            <EmptyState text={recursive ? 'This folder has no photos or videos.' : 'No photos directly in this folder. Turn on subfolders to see everything below it.'} />
          {/snippet}
        </Grid>
      </div>
    {:else}
      <EmptyState text="Add a library folder in Settings to browse it here." />
    {/if}
  </div>
</div>

<style>
  .folders {
    display: flex;
    height: 100%;
  }
  .tree {
    width: 260px;
    flex: none;
    overflow: auto;
    padding: 8px;
    border-right: 1px solid var(--border);
    font-size: var(--t13);
  }
  .node {
    display: flex;
    align-items: center;
    height: 28px;
    border-radius: var(--r-1);
  }
  .node.active {
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .twisty {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    flex: none;
    padding: 0;
    border: 0;
    background: none;
    color: var(--muted);
    cursor: pointer;
    transition: transform 120ms var(--ease);
  }
  .twisty.open {
    transform: rotate(90deg);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0 6px 0 2px;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .name span:first-child {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    color: var(--muted);
    font-size: var(--t12);
  }
  .pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 44px;
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--border);
    font-size: var(--t13);
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }
  .grid {
    flex: 1;
    min-height: 0;
  }
</style>
