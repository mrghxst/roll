<script lang="ts">
  import { ExternalLink, FolderOpen } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { viewer } from '../state/viewer.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Activity from '../views/Activity.svelte';
  import Folders from '../views/Folders.svelte';
  import SettingsView from '../views/Settings.svelte';
  import TimelineView from '../views/Timeline.svelte';
  import Viewer from '../viewer/Viewer.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import FilterPanel from './FilterPanel.svelte';
  import InfoPanel from './InfoPanel.svelte';
  import Sidebar from './Sidebar.svelte';
  import TopBar from './TopBar.svelte';

  let topbar: ReturnType<typeof TopBar> | undefined = $state();

  function typing(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    return t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable;
  }

  function onkeydown(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      app.paletteOpen = !app.paletteOpen;
      return;
    }
    if (typing(e) || viewer.ids || app.paletteOpen) return;
    if (e.key === '/') {
      e.preventDefault();
      topbar?.focusSearch();
    } else if (e.key === 'i' || e.key === 'I') {
      if (!mod) app.infoOpen = !app.infoOpen;
    }
  }

  const single = $derived(selection.size === 1 ? selection.list()[0] : null);
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Sidebar />
  <div class="main">
    <TopBar bind:this={topbar}>
      {#snippet selectionActions()}
        {#if single != null}
          <IconButton label="Show in Explorer" onclick={() => void api.showInFolder(single)}><FolderOpen size={16} strokeWidth={1.5} /></IconButton>
          <IconButton label="Open with the default app" onclick={() => void api.openWithDefault(single)}><ExternalLink size={16} strokeWidth={1.5} /></IconButton>
        {/if}
      {/snippet}
    </TopBar>
    <div class="body">
      {#if app.filtersOpen && (app.view === 'timeline' || app.view === 'folders')}
        <FilterPanel />
      {/if}
      <section class="content">
        {#if app.view === 'timeline'}
          <TimelineView />
        {:else if app.view === 'folders'}
          <Folders />
        {:else if app.view === 'activity'}
          <Activity />
        {:else if app.view === 'settings'}
          <SettingsView />
        {/if}
      </section>
      {#if app.infoOpen}
        <InfoPanel id={single ?? null} />
      {/if}
    </div>
  </div>
</div>

{#if viewer.ids}
  <Viewer />
{/if}
<CommandPalette onsearch={() => topbar?.focusSearch()} />

<style>
  .shell {
    display: flex;
    height: 100%;
  }
  .main {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .content {
    flex: 1;
    min-width: 0;
    position: relative;
  }
</style>
