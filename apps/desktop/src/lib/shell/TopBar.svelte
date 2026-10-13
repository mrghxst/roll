<script lang="ts">
  import { Bot, Command, Info, ListFilter, Search, X } from '@lucide/svelte';
  import type { Snippet } from 'svelte';
  import { api, emptyQuery } from '../ipc/api';
  import type { ParsedSearch } from '../ipc/bindings/ParsedSearch';
  import type { Query } from '../ipc/bindings/Query';
  import { app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { toast } from '../state/toast.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Kbd from '../ui/Kbd.svelte';
  import { n } from '../util/format';
  import { chips, removeChip } from './chips';

  let { selectionActions }: { selectionActions?: Snippet } = $props();

  let input: HTMLInputElement | undefined = $state();
  let text = $state(app.filters.text ?? '');
  let timer = 0;
  /** Words the parser left, offered to the text model when AI is on. */
  let askable = $state<string | null>(null);
  let asking = $state(false);

  $effect(() => {
    text = app.filters.text ?? '';
  });

  function oninput() {
    askable = null;
    clearTimeout(timer);
    timer = window.setTimeout(() => {
      app.filters = { ...app.filters, text: text.trim() || null };
    }, 180);
  }

  /** Enter: words the parser understands become chips; the rest stays. */
  async function understand() {
    const t = text.trim();
    if (!t) return;
    clearTimeout(timer);
    const p = await api.parseSearch(t);
    askable = app.settings && app.settings.ai_mode !== 'off' && p.rest.trim() ? p.rest.trim() : null;
    if (!p.understood.length) {
      app.filters = { ...app.filters, text: t };
      return;
    }
    apply(p);
  }

  /** The text model reads the leftover words; its filters become chips. */
  async function ask() {
    if (!askable || asking) return;
    asking = true;
    try {
      const p = await api.aiSearch(askable);
      askable = null;
      if (!p.understood.length) toast.show('The model found no filters in those words. They stay as search text.');
      else apply(p);
    } catch (e) {
      toast.error(e);
    } finally {
      asking = false;
    }
  }

  function apply(p: ParsedSearch) {
    const empty = emptyQuery();
    const next: Partial<Query> = { ...app.filters };
    for (const [k, v] of Object.entries(p.query) as [keyof Query, unknown][]) {
      const base = empty[k] as unknown;
      if (Array.isArray(v)) {
        if (v.length) (next[k] as unknown) = [...new Set([...((app.filters[k] as unknown[]) ?? []), ...v])];
      } else if (v != null && v !== base) (next[k] as unknown) = v;
    }
    const labels = { ...app.filterLabels };
    const near = p.understood.find((u) => u.startsWith('Near '));
    if (p.query.bbox && near) labels.bbox = near;
    const dated = p.understood.find((u) => /\d{4}|year|month/i.test(u));
    if (p.query.from_local && dated) labels.local = dated;
    text = p.rest;
    next.text = p.rest || null;
    app.filterLabels = labels;
    app.filters = next;
  }

  export function focusSearch() {
    input?.focus();
    input?.select();
  }

  const active = $derived(chips(app.filters, app.filterLabels));
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
        if (e.key === 'Enter') {
          e.preventDefault();
          void understand();
        } else if (e.key === 'Escape') {
          text = '';
          app.filters = { ...app.filters, text: null };
          input?.blur();
        } else if (e.key === 'Backspace' && !text && active.length) {
          app.filters = removeChip(app.filters, active[active.length - 1]!.key);
        }
      }}
      placeholder="Search: “videos summer 2019 Italy”, a file name, a tag…"
      aria-label="Search"
      spellcheck="false"
      data-testid="search"
    />
    {#if askable}
      <button
        class="ask"
        disabled={asking}
        onclick={() => void ask()}
        title={app.settings?.ai_mode === 'openrouter' ? 'Sends these words, not your photos, to OpenRouter' : 'Asks the local model'}
        data-testid="ask-ai"
      >
        <Bot size={14} strokeWidth={1.5} />
        {asking ? 'Asking…' : 'Ask AI about these words'}
      </button>
    {/if}
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
  .ask {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    border: 1px dashed var(--border);
    border-radius: var(--r-1);
    background: transparent;
    color: var(--fg);
    font-size: var(--t12);
    white-space: nowrap;
    cursor: pointer;
  }
  .ask:hover {
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
