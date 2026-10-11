<script lang="ts">
  import { Command, Info, ListFilter, Search, X } from '@lucide/svelte';
  import type { Snippet } from 'svelte';
  import { app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Kbd from '../ui/Kbd.svelte';
  import { n } from '../util/format';
  import { chips, removeChip } from './chips';

  let { selectionActions }: { selectionActions?: Snippet } = $props();

  let input: HTMLInputElement | undefined = $state();
  let text = $state(app.filters.text ?? '');
  let timer = 0;

  $effect(() => {
    text = app.filters.text ?? '';
  });

  function oninput() {
    clearTimeout(timer);
    timer = window.setTimeout(() => {
      app.filters = { ...app.filters, text: text.trim() || null };
    }, 180);
  }

  export function focusSearch() {
    input?.focus();
    input?.select();
  }

  const active = $derived(chips(app.filters));
  const showGridOptions = $derived(app.view === 'timeline' || app.view === 'folders');
</script>

<header class="topbar">
  <div class="search" class:has={active.length > 0}>
    <Search size={16} strokeWidth={1.5} />
    {#each active as c (c.key)}
      <span class="chip">
        {c.label}
        <button aria-label="Remove filter {c.label}" onclick={() => (app.filters = removeChip(app.filters, c.key))}><X size={12} strokeWidth={2} /></button>
      </span>
    {/each}
    <input
      bind:this={input}
      bind:value={text}
      {oninput}
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          text = '';
          app.filters = { ...app.filters, text: null };
          input?.blur();
        } else if (e.key === 'Backspace' && !text && active.length) {
          app.filters = removeChip(app.filters, active[active.length - 1]!.key);
        }
      }}
      placeholder="Search photos, places, text…"
      aria-label="Search"
      spellcheck="false"
      data-testid="search"
    />
  </div>
  <IconButton label="Filters" active={app.filtersOpen} onclick={() => (app.filtersOpen = !app.filtersOpen)} data-testid="filters-toggle">
    <ListFilter size={16} strokeWidth={1.5} />
  </IconButton>
  <div class="spacer"></div>

  {#if selection.size > 0}
    <div class="selection" data-testid="selection-bar">
      <span class="mono">{n(selection.size)} selected</span>
      {@render selectionActions?.()}
      <IconButton label="Clear selection" onclick={() => selection.clear()}><X size={16} strokeWidth={1.5} /></IconButton>
    </div>
  {:else if showGridOptions && app.settings}
    <label class="size" title="Row height (Ctrl + scroll)">
      <span class="sr-only">Row height</span>
      <input
        type="range"
        min="100"
        max="420"
        step="10"
        value={app.settings.row_height}
        oninput={(e) => {
          if (app.settings) app.settings.row_height = Number(e.currentTarget.value);
        }}
        onchange={(e) => void app.saveSettings({ row_height: Number(e.currentTarget.value) })}
      />
    </label>
  {/if}
  <IconButton label="Info (I)" active={app.infoOpen} onclick={() => (app.infoOpen = !app.infoOpen)}>
    <Info size={16} strokeWidth={1.5} />
  </IconButton>
  <button class="palette" onclick={() => (app.paletteOpen = true)} aria-label="Command palette">
    <Command size={14} strokeWidth={1.5} />
    <Kbd keys="Ctrl+K" />
  </button>
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--topbar);
    padding: 0 12px 0 16px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    width: min(520px, 50vw);
    min-width: 160px;
    flex-shrink: 1;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--surface);
    color: var(--muted);
    transition: border-color 120ms var(--ease);
  }
  .search:focus-within {
    border-color: var(--fg);
    background: var(--bg);
  }
  .search input {
    flex: 1;
    min-width: 80px;
    height: 100%;
    border: 0;
    background: transparent;
    outline: none;
    color: var(--fg);
    font-size: var(--t13);
    user-select: text;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 4px 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t12);
    white-space: nowrap;
  }
  .chip button {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    border-radius: 3px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .chip button:hover {
    color: var(--fg);
    background: var(--surface);
  }
  .spacer {
    flex: 1;
  }
  .selection {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--t13);
  }
  .selection .mono {
    margin-right: 8px;
    white-space: nowrap;
  }
  .size input {
    width: 96px;
    accent-color: var(--fg);
  }
  .palette {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .palette:hover {
    background: var(--surface);
  }
</style>
