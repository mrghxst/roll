<script lang="ts">
  import {
    Activity,
    Bot,
    Cloud,
    AtSign,
    CalendarClock,
    CalendarRange,
    Compass,
    Copy,
    Sparkles,
    ListChecks,
    Ban,
    CircleDashed,
    FileText,
    Folder,
    FolderInput,
    FolderTree,
    History,
    Image,
    Images,
    Map,
    MapPin,
    PanelLeftClose,
    PanelLeftOpen,
    Settings,
    Smartphone,
    Smile,
    Tag,
    Trash2,
    Users,
    Wallpaper,
  } from '@lucide/svelte';
  import type { Component } from 'svelte';
  import { ALL, app, type Scope, type View } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { compact } from '../util/format';

  const CATEGORY_ICONS: Record<string, Component> = {
    photos: Image,
    screenshots: Smartphone,
    memes: Smile,
    social_media: AtSign,
    wallpapers: Wallpaper,
    documents: FileText,
    junk: Ban,
    unsorted: CircleDashed,
  };

  const collapsed = $derived(app.sidebarCollapsed);
  const activeJob = $derived(app.activeJobs[0]);

  function goScope(scope: Scope) {
    selection.clear();
    app.go('timeline', scope);
  }

  function goView(view: View) {
    selection.clear();
    app.go(view);
  }

  const isScope = (key: string) => app.view === 'timeline' && app.scope.key === key;
  const topCategories = $derived(app.categories.filter((c) => c.parent_id === null && (c.count > 0 || c.key === 'photos')));
</script>

{#snippet item(label: string, Icon: Component, active: boolean, onclick: () => void, count?: number, testid?: string)}
  <button class="item" class:active {onclick} title={collapsed ? label : undefined} aria-current={active ? 'page' : undefined} data-testid={testid}>
    <Icon size={16} strokeWidth={1.5} />
    {#if !collapsed}
      <span class="label">{label}</span>
      {#if count}<span class="count mono">{compact(count)}</span>{/if}
    {/if}
  </button>
{/snippet}

<nav class="sidebar" class:collapsed aria-label="Sections">
  <div class="top">
    {#if !collapsed}<span class="brand">Roll</span>{/if}
    <button class="collapse" aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'} onclick={() => (app.sidebarCollapsed = !collapsed)}>
      {#if collapsed}<PanelLeftOpen size={16} strokeWidth={1.5} />{:else}<PanelLeftClose size={16} strokeWidth={1.5} />{/if}
    </button>
  </div>
  <div class="scroll">
    {#if app.settings?.first_run_done && !app.settings.guide_done}
      {@render item('Get started', Compass, app.view === 'guide', () => goView('guide'), undefined, 'nav-guide')}
    {/if}
    {@render item('Timeline', Images, isScope('all'), () => goScope(ALL), undefined, 'nav-timeline')}
    {@render item('Folders', Folder, app.view === 'folders', () => goView('folders'), undefined, 'nav-folders')}
    {@render item('Map', Map, app.view === 'map', () => goView('map'), undefined, 'nav-map')}
    {@render item('Events', CalendarRange, app.view === 'events', () => goView('events'), undefined, 'nav-events')}
    {@render item('People', Users, app.view === 'people', () => goView('people'), undefined, 'nav-people')}
    {@render item('Add photos', FolderInput, app.view === 'add', () => goView('add'), undefined, 'nav-add')}

    {#if !collapsed}<div class="section">Fix</div>{:else}<hr />{/if}
    {@render item('Review', ListChecks, app.view === 'review', () => goView('review'), app.counts.review, 'nav-review')}
    {@render item('Discover', Sparkles, app.view === 'discover', () => goView('discover'), undefined, 'nav-discover')}
    {#if app.settings && app.settings.ai_mode !== 'off'}
      {@render item('Sort with AI', Bot, app.view === 'ai', () => goView('ai'), undefined, 'nav-ai')}
    {/if}
    {@render item('Duplicates', Copy, app.view === 'duplicates', () => goView('duplicates'), app.counts.duplicates, 'nav-duplicates')}
    {@render item('Dates', CalendarClock, app.view === 'dates', () => goView('dates'), app.counts.date_issues, 'nav-dates')}
    {@render item('Places', MapPin, app.view === 'places', () => goView('places'), app.counts.no_location, 'nav-places')}

    {#if !collapsed}<div class="section">Library</div>{:else}<hr />{/if}
    {#each topCategories as c (c.id)}
      {@render item(
        c.name,
        CATEGORY_ICONS[c.key] ?? Tag,
        isScope(`cat:${c.key}`),
        () => goScope({ key: `cat:${c.key}`, label: c.name, query: { categories: [c.key] } }),
        c.count,
      )}
    {/each}

    {#if app.tags.length && !collapsed}
      <div class="section">Tags</div>
      {#each app.tags.filter((t) => !t.path.includes('/') || t.path.startsWith('Event/') || t.path.startsWith('Source/')).slice(0, 40) as t (t.id)}
        {@render item(t.path, Tag, isScope(`tag:${t.path}`), () => goScope({ key: `tag:${t.path}`, label: t.path, query: { tags: [t.path] } }), t.count)}
      {/each}
    {/if}
  </div>
  <div class="bottom">
    {@render item('Organize', FolderTree, app.view === 'organize', () => app.go('organize'), app.unsaved, 'nav-organize')}
    {@render item('Immich', Cloud, app.view === 'immich', () => goView('immich'), undefined, 'nav-immich')}
    {@render item('History', History, app.view === 'history', () => goView('history'), undefined, 'nav-history')}
    {@render item('Trash', Trash2, app.view === 'trash', () => goView('trash'), app.counts.trash, 'nav-trash')}
    <button class="item" class:active={app.view === 'activity'} onclick={() => goView('activity')} title={collapsed ? 'Activity' : undefined} data-testid="nav-activity">
      <Activity size={16} strokeWidth={1.5} />
      {#if !collapsed}
        <span class="label">Activity</span>
        {#if activeJob}
          <span class="job mono" title={activeJob.label}>{activeJob.total ? `${Math.floor((activeJob.done / activeJob.total) * 100)}%` : '…'}</span>
        {/if}
      {:else if activeJob}
        <span class="dot" aria-label="Working"></span>
      {/if}
    </button>
    {@render item('Settings', Settings, app.view === 'settings', () => goView('settings'), undefined, 'nav-settings')}
  </div>
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    width: var(--sidebar);
    height: 100%;
    border-right: 1px solid var(--border);
    background: var(--surface);
    transition: width 160ms var(--ease);
  }
  .collapsed {
    width: var(--sidebar-collapsed);
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--topbar);
    padding: 0 8px 0 16px;
    flex: none;
  }
  .collapsed .top {
    justify-content: center;
    padding: 0;
  }
  .brand {
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .collapse {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .collapse:hover {
    background: var(--bg);
    color: var(--fg);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px;
  }
  .bottom {
    padding: 8px;
    border-top: 1px solid var(--border);
  }
  .section {
    margin: 16px 8px 4px;
    color: var(--muted);
    font-size: var(--t12);
  }
  hr {
    margin: 8px 4px;
    border: 0;
    border-top: 1px solid var(--border);
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--muted);
    font-size: var(--t13);
    text-align: left;
    cursor: pointer;
  }
  .collapsed .item {
    justify-content: center;
    padding: 0;
  }
  .item:hover {
    color: var(--fg);
    background: color-mix(in srgb, var(--fg) 5%, transparent);
  }
  .item.active {
    color: var(--fg);
    background: var(--bg);
    box-shadow: 0 0 0 1px var(--border);
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count,
  .job {
    color: var(--muted);
    font-size: var(--t12);
  }
  .dot {
    position: absolute;
    top: 6px;
    right: 10px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--fg);
  }
</style>
