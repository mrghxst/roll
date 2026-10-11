<script lang="ts">
  import { app } from '../state/app.svelte';
  import { toggleFavorite, trash, undoLast } from '../state/edits';
  import { selection } from '../state/selection.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Toasts from '../ui/Toasts.svelte';
  import Activity from '../views/Activity.svelte';
  import Folders from '../views/Folders.svelte';
  import History from '../views/History.svelte';
  import Dates from '../views/Dates.svelte';
  import Discover from '../views/Discover.svelte';
  import Events from '../views/Events.svelte';
  import Duplicates from '../views/Duplicates.svelte';
  import Review from '../views/Review.svelte';
  import MapPage from '../views/MapPage.svelte';
  import Places from '../views/Places.svelte';
  import Organize from '../views/Organize.svelte';
  import SettingsView from '../views/Settings.svelte';
  import TimelineView from '../views/Timeline.svelte';
  import Trash from '../views/Trash.svelte';
  import Viewer from '../viewer/Viewer.svelte';
  import CommandPalette from './CommandPalette.svelte';
  import FilterPanel from './FilterPanel.svelte';
  import InfoPanel from './InfoPanel.svelte';
  import RecycleSheet from './RecycleSheet.svelte';
  import PackSheet from './PackSheet.svelte';
  import SelectionActions from './SelectionActions.svelte';
  import Sidebar from './Sidebar.svelte';
  import TagSheet from './TagSheet.svelte';
  import ZoneSheet from './ZoneSheet.svelte';
  import DateSheet from './DateSheet.svelte';
  import LocationSheet from './LocationSheet.svelte';
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
    if (typing(e) || viewer.ids || app.paletteOpen || app.tagging || app.recycling || app.locating || app.dating) return;
    if (document.querySelector('dialog[open]')) return;
    const key = e.key.toLowerCase();
    if (mod && key === 'z' && !e.shiftKey) {
      e.preventDefault();
      void undoLast();
      return;
    }
    if (mod || e.altKey) return;
    const ids = selection.list();
    const grid = app.view === 'timeline' || app.view === 'folders';
    if (e.key === '/') {
      e.preventDefault();
      topbar?.focusSearch();
    } else if (key === 'i') {
      app.infoOpen = !app.infoOpen;
    } else if (e.key === 'Delete' && ids.length && grid) {
      e.preventDefault();
      void trash(ids);
      selection.clear();
    } else if (e.key === 'Delete' && ids.length && app.view === 'trash') {
      e.preventDefault();
      app.recycling = ids;
    } else if (key === 'f' && ids.length && grid) {
      void toggleFavorite(ids);
    } else if (key === 't' && ids.length && grid) {
      e.preventDefault();
      app.tagging = ids;
    } else if (key === 'l' && ids.length && grid) {
      e.preventDefault();
      app.locating = ids;
    } else if (key === 'd' && ids.length && grid) {
      e.preventDefault();
      app.dating = ids;
    }
  }

  const single = $derived(selection.size === 1 ? selection.list()[0] : null);
  const gridView = $derived(app.view === 'timeline' || app.view === 'folders' || app.view === 'trash');
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Sidebar />
  <div class="main">
    <TopBar bind:this={topbar}>
      {#snippet selectionActions()}
        {#if gridView}
          <SelectionActions onemptytrash={(ids) => (app.recycling = ids)} />
        {/if}
      {/snippet}
    </TopBar>
    <div class="body">
      {#if app.filtersOpen && gridView}
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
        {:else if app.view === 'organize'}
          <Organize />
        {:else if app.view === 'history'}
          <History />
        {:else if app.view === 'trash'}
          <Trash />
        {:else if app.view === 'map'}
          <MapPage />
        {:else if app.view === 'dates'}
          <Dates />
        {:else if app.view === 'review'}
          <Review />
        {:else if app.view === 'duplicates'}
          <Duplicates />
        {:else if app.view === 'events'}
          <Events />
        {:else if app.view === 'discover'}
          <Discover />
        {:else if app.view === 'places'}
          <Places />
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
<TagSheet />
<RecycleSheet />
<ZoneSheet />
<LocationSheet />
<DateSheet />
<PackSheet />
<Toasts />

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
