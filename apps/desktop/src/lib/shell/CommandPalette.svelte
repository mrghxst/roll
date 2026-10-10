<script lang="ts">
  import { api } from '../ipc/api';
  import { ALL, app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import Kbd from '../ui/Kbd.svelte';

  let { onsearch }: { onsearch: () => void } = $props();

  interface Cmd {
    title: string;
    group: string;
    keys?: string;
    run: () => void;
  }

  let text = $state('');
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let dialog: HTMLDialogElement | undefined = $state();

  const commands = $derived.by<Cmd[]>(() => {
    const list: Cmd[] = [
      { group: 'Go to', title: 'Timeline', run: () => app.go('timeline', ALL) },
      { group: 'Go to', title: 'Folders', run: () => app.go('folders') },
      { group: 'Go to', title: 'Activity', run: () => app.go('activity') },
      { group: 'Go to', title: 'Settings', run: () => app.go('settings') },
      ...app.categories
        .filter((c) => c.count > 0)
        .map((c) => ({ group: 'Go to', title: c.name, run: () => app.go('timeline', { key: `cat:${c.key}`, label: c.name, query: { categories: [c.key] } }) })),
      { group: 'View', title: 'Search', keys: '/', run: onsearch },
      { group: 'View', title: app.infoOpen ? 'Hide info' : 'Show info', keys: 'I', run: () => (app.infoOpen = !app.infoOpen) },
      { group: 'View', title: app.filtersOpen ? 'Hide filters' : 'Show filters', run: () => (app.filtersOpen = !app.filtersOpen) },
      { group: 'View', title: app.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar', run: () => (app.sidebarCollapsed = !app.sidebarCollapsed) },
      { group: 'View', title: 'Clear filters', run: () => (app.filters = {}) },
      { group: 'View', title: 'Theme: system', run: () => void app.saveSettings({ theme: 'system' }) },
      { group: 'View', title: 'Theme: light', run: () => void app.saveSettings({ theme: 'light' }) },
      { group: 'View', title: 'Theme: dark', run: () => void app.saveSettings({ theme: 'dark' }) },
      { group: 'Filter', title: 'Videos', run: () => (app.filters = { ...app.filters, media: ['video'] }) },
      { group: 'Filter', title: 'Favorites', run: () => (app.filters = { ...app.filters, favorite: true }) },
      { group: 'Filter', title: 'No location', run: () => (app.filters = { ...app.filters, location: 'none' }) },
      { group: 'Filter', title: 'Date problems', run: () => (app.filters = { ...app.filters, problems: 3 }) },
      { group: 'Library', title: 'Check folders for changes', run: () => void api.rescan() },
      {
        group: 'Library',
        title: 'Add folder…',
        run: async () => {
          const p = await api.pickFolder('Add a library folder');
          if (p) await api.addRoot(p);
        },
      },
    ];
    if (selection.size) {
      list.unshift({ group: 'Selection', title: 'Clear selection', keys: 'Esc', run: () => selection.clear() });
      if (selection.size === 1) {
        const id = selection.list()[0]!;
        list.unshift(
          { group: 'Selection', title: 'Show in Explorer', run: () => void api.showInFolder(id) },
          { group: 'Selection', title: 'Open with the default app', run: () => void api.openWithDefault(id) },
        );
      }
    }
    return list;
  });

  const filtered = $derived.by(() => {
    const t = text.trim().toLowerCase();
    if (!t) return commands;
    const words = t.split(/\s+/);
    return commands.filter((c) => words.every((w) => `${c.group} ${c.title}`.toLowerCase().includes(w)));
  });

  $effect(() => {
    if (!dialog) return;
    if (app.paletteOpen && !dialog.open) {
      text = '';
      active = 0;
      dialog.showModal();
      queueMicrotask(() => input?.focus());
    } else if (!app.paletteOpen && dialog.open) dialog.close();
  });

  $effect(() => {
    void text;
    active = 0;
  });

  function run(c: Cmd | undefined) {
    if (!c) return;
    app.paletteOpen = false;
    c.run();
  }
</script>

<dialog
  bind:this={dialog}
  class="palette"
  aria-label="Command palette"
  onclose={() => (app.paletteOpen = false)}
  onclick={(e) => {
    if (e.target === dialog) app.paletteOpen = false;
  }}
>
  <input
    bind:this={input}
    bind:value={text}
    placeholder="Type a command…"
    aria-label="Command"
    spellcheck="false"
    onkeydown={(e) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        active = Math.min(filtered.length - 1, active + 1);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        active = Math.max(0, active - 1);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        run(filtered[active]);
      }
    }}
  />
  <ul role="listbox" aria-label="Commands">
    {#each filtered as c, i (c.group + c.title)}
      <li role="option" aria-selected={i === active}>
        <button class:active={i === active} onmouseenter={() => (active = i)} onclick={() => run(c)}>
          <span class="group">{c.group}</span>
          <span class="title">{c.title}</span>
          {#if c.keys}<Kbd keys={c.keys} />{/if}
        </button>
      </li>
    {:else}
      <li class="none">No matching command.</li>
    {/each}
  </ul>
</dialog>

<style>
  .palette {
    width: 560px;
    max-width: calc(100vw - 32px);
    margin-top: 14vh;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--bg);
    color: var(--fg);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.2);
  }
  .palette::backdrop {
    background: var(--overlay);
  }
  input {
    width: 100%;
    height: 48px;
    padding: 0 16px;
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    outline: none;
    font-size: var(--t16);
    user-select: text;
  }
  ul {
    max-height: 360px;
    margin: 0;
    padding: 6px;
    overflow-y: auto;
    list-style: none;
  }
  li button {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: none;
    text-align: left;
    cursor: pointer;
    font-size: var(--t14);
  }
  li button.active {
    background: var(--surface);
  }
  .group {
    width: 72px;
    color: var(--muted);
    font-size: var(--t12);
  }
  .title {
    flex: 1;
  }
  .none {
    padding: 16px;
    color: var(--muted);
    font-size: var(--t13);
  }
</style>
