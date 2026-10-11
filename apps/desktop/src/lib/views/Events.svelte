<script lang="ts">
  import { Check, FolderTree, Images, Merge, Pencil, Split, X } from '@lucide/svelte';
  import { api, emptyQuery } from '../ipc/api';
  import type { EventInfo } from '../ipc/bindings/EventInfo';
  import type { PlaceHit } from '../ipc/bindings/PlaceHit';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import Input from '../ui/Input.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import Switch from '../ui/Switch.svelte';
  import { coords, n, shortDate } from '../util/format';

  let events = $state<EventInfo[]>([]);
  let loaded = $state(false);
  let renaming = $state<{ id: number; name: string; confirm: boolean } | null>(null);
  let splitting = $state<{ ev: EventInfo; ids: number[]; at: number | null; name: string } | null>(null);
  let merging = $state<number[]>([]);
  let homeQuery = $state('');
  let homeHits = $state<PlaceHit[]>([]);
  let settingHome = $state(false);

  $effect(() => {
    void app.revision;
    void app.historyRevision;
    void api
      .events()
      .then((e) => {
        events = e;
        loaded = true;
      })
      .catch((e) => toast.error(e));
  });

  const proposed = $derived(events.filter((e) => e.state === 'proposed'));
  const confirmed = $derived(events.filter((e) => e.state === 'confirmed'));

  function range(e: EventInfo) {
    const a = shortDate(e.start_local);
    const b = shortDate(e.end_local);
    return a === b ? a : `${a} – ${b}`;
  }

  async function run(f: () => Promise<unknown>, done?: string) {
    try {
      await f();
      if (done) toast.show(done);
    } catch (e) {
      toast.error(e);
    }
  }

  function open(e: EventInfo) {
    app.go('timeline', { key: `event:${e.id}`, label: e.name, query: { event_id: e.id } });
  }

  async function startSplit(ev: EventInfo) {
    const ids = await api.queryIds({ ...emptyQuery(), event_id: ev.id });
    splitting = { ev, ids, at: null, name: `${ev.name} (2)` };
  }

  function toggleMerge(id: number) {
    merging = merging.includes(id) ? merging.filter((x) => x !== id) : [...merging, id];
  }

  async function searchHome() {
    homeHits = homeQuery.trim() ? (await api.placeSearch(homeQuery)).slice(0, 6) : [];
  }

  const home = $derived(app.settings?.home && app.settings.home.length === 2 ? app.settings.home : null);
  let homeLabel = $state<string | null>(null);
  $effect(() => {
    const h = home;
    if (h) void api.placeName(h[0]!, h[1]!).then((l) => (homeLabel = l ?? coords(h[0]!, h[1]!)));
  });
</script>

{#snippet card(e: EventInfo)}
  <li data-testid="event" data-event={e.id}>
    <div class="head">
      {#if e.state === 'proposed'}
        <input type="checkbox" checked={merging.includes(e.id)} onchange={() => toggleMerge(e.id)} aria-label="Select {e.name} to merge" />
      {/if}
      <strong>{e.name}</strong>
      <span class="muted">{range(e)} · {n(e.count)} {e.count === 1 ? 'item' : 'items'}{e.away ? ' · away from home' : ''}</span>
    </div>
    <div class="samples">
      {#each e.samples as id, i (id)}
        <button class="s" ondblclick={() => viewer.open(Uint32Array.from(e.samples), i, null)} aria-label="Open"><img src={api.thumbUrl(id, 1)} alt="" loading="lazy" /></button>
      {/each}
    </div>
    <div class="actions">
      {#if e.state === 'proposed'}
        <Button variant="primary" onclick={() => void run(() => api.eventConfirm(e.id, null), `Confirmed ${e.name}`)} data-testid="confirm-event">
          <Check size={14} strokeWidth={1.5} /> Confirm
        </Button>
        <Button onclick={() => (renaming = { id: e.id, name: e.name, confirm: true })}><Pencil size={14} strokeWidth={1.5} /> Rename and confirm…</Button>
      {:else}
        <Button onclick={() => (renaming = { id: e.id, name: e.name, confirm: false })}><Pencil size={14} strokeWidth={1.5} /> Rename…</Button>
      {/if}
      <Button variant="ghost" onclick={() => open(e)}><Images size={14} strokeWidth={1.5} /> Show items</Button>
      <Button variant="ghost" onclick={() => void startSplit(e)}><Split size={14} strokeWidth={1.5} /> Split…</Button>
      <Button variant="ghost" onclick={() => void run(() => api.eventDismiss(e.id), e.state === 'confirmed' ? `Removed ${e.name}` : undefined)}>
        <X size={14} strokeWidth={1.5} />
        {e.state === 'confirmed' ? 'Remove event' : 'Not an event'}
      </Button>
      {#if e.state === 'confirmed'}
        <span class="folder">
          <Switch label="Own folder" checked={e.own_folder} onchange={(v) => void run(async () => { await api.eventOwnFolder(e.id, v); e.own_folder = v; })} />
          <span class="muted"><FolderTree size={12} strokeWidth={1.5} /> Own folder when organizing</span>
        </span>
      {/if}
    </div>
  </li>
{/snippet}

<div class="events">
  <header>
    <h1>Events</h1>
    <p class="lead">
      Photos that belong together: trips away from home and busy days at home. Confirming tags the items
      <span class="mono">Event/name</span> and, when you organize, gives them a folder such as
      <span class="mono">2019/2019-08 August 2019 Italy</span>.
    </p>
    <div class="home">
      <span class="muted">
        Home: {home ? (homeLabel ?? coords(home[0]!, home[1]!)) : 'found from where most evening photos were taken'}. Away means more than 50 km
        from it.
      </span>
      {#if settingHome}
        <Input bind:value={homeQuery} placeholder="Search a town or paste coordinates" aria-label="Home" oninput={() => void searchHome()} />
        <Button variant="ghost" onclick={() => (settingHome = false)}>Cancel</Button>
      {:else}
        <Button variant="ghost" onclick={() => (settingHome = true)}>Set home…</Button>
        {#if home}<Button variant="ghost" onclick={() => void app.saveSettings({ home: null })}>Find home automatically</Button>{/if}
      {/if}
    </div>
    {#if settingHome && homeHits.length}
      <ul class="hits">
        {#each homeHits as h (h.label + h.lat)}
          <li>
            <button
              onclick={async () => {
                await app.saveSettings({ home: [h.lat, h.lon] });
                settingHome = false;
                homeQuery = '';
                homeHits = [];
              }}>{h.label}</button
            >
          </li>
        {/each}
      </ul>
    {/if}
    {#if merging.length >= 2}
      <div class="mergebar" role="region" aria-label="Merge">
        <span>{merging.length} events selected</span>
        <Button
          variant="primary"
          onclick={() =>
            void run(async () => {
              await api.eventMerge(merging);
              merging = [];
            }, 'Merged into one event')}
          data-testid="merge-events"><Merge size={14} strokeWidth={1.5} /> Merge into one</Button
        >
        <Button variant="ghost" onclick={() => (merging = [])}>Clear</Button>
      </div>
    {/if}
  </header>

  {#if loaded && !events.length}
    <EmptyState text="No events yet. Events need at least 8 photos close together in time, sorted as Photos, with real capture dates." />
  {:else}
    {#if proposed.length}
      <h2>Suggested ({n(proposed.length)})</h2>
      <ul class="list" data-testid="proposed-events">
        {#each proposed as e (e.id)}{@render card(e)}{/each}
      </ul>
    {/if}
    {#if confirmed.length}
      <h2>Confirmed ({n(confirmed.length)})</h2>
      <ul class="list" data-testid="confirmed-events">
        {#each confirmed as e (e.id)}{@render card(e)}{/each}
      </ul>
    {/if}
  {/if}
</div>

<Sheet open={renaming != null} title={renaming?.confirm ? 'Rename and confirm' : 'Rename event'} width={440} onclose={() => (renaming = null)}>
  {#if renaming}
    <label class="field"><span>Name</span><Input bind:value={renaming.name} aria-label="Event name" /></label>
    <p class="muted">The items get the tag <span class="mono">Event/{renaming.name.trim()}</span>.</p>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (renaming = null)}>Cancel</Button>
    <Button
      variant="primary"
      disabled={!renaming?.name.trim()}
      onclick={() => {
        const r = renaming!;
        renaming = null;
        void run(() => (r.confirm ? api.eventConfirm(r.id, r.name.trim()) : api.eventRename(r.id, r.name.trim())), r.confirm ? `Confirmed ${r.name.trim()}` : undefined);
      }}
      data-testid="save-event-name">{renaming?.confirm ? 'Confirm' : 'Rename'}</Button
    >
  {/snippet}
</Sheet>

<Sheet open={splitting != null} title="Split {splitting?.ev.name ?? ''}" width={720} onclose={() => (splitting = null)}>
  {#if splitting}
    <p class="muted">Click the first item of the new event. It and everything after it move to the new event.</p>
    <div class="pick">
      {#each splitting.ids as id, i (id)}
        <button
          class="s"
          class:after={splitting.at != null && i >= splitting.ids.indexOf(splitting.at)}
          disabled={i === 0}
          onclick={() => (splitting!.at = id)}
          aria-label="Start the new event here"><img src={api.thumbUrl(id, 1)} alt="" loading="lazy" /></button
        >
      {/each}
    </div>
    <label class="field"><span>Name of the new event</span><Input bind:value={splitting.name} aria-label="Name of the new event" /></label>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (splitting = null)}>Cancel</Button>
    <Button
      variant="primary"
      disabled={splitting?.at == null || !splitting?.name.trim()}
      onclick={() => {
        const s = splitting!;
        splitting = null;
        void run(() => api.eventSplit(s.ev.id, s.at!, s.name.trim()), `Split off ${s.name.trim()}`);
      }}
      data-testid="split-event"
      >Split off {splitting?.at != null ? n(splitting.ids.length - splitting.ids.indexOf(splitting.at)) : ''} items</Button
    >
  {/snippet}
</Sheet>

<style>
  .events {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  header,
  .list,
  h2 {
    max-width: 980px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  h2 {
    margin: 24px 0 10px;
    font-size: var(--t14);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    max-width: 72ch;
  }
  .home,
  .mergebar {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t13);
  }
  .mergebar {
    margin-top: 12px;
    padding: 8px 12px;
    border-radius: var(--r-btn);
    background: var(--surface);
  }
  .hits {
    margin: 6px 0 0;
    padding: 0;
    list-style: none;
    max-width: 480px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  .hits button {
    width: 100%;
    padding: 6px 10px;
    border: 0;
    background: none;
    color: var(--fg);
    text-align: left;
    font-size: var(--t13);
    cursor: pointer;
  }
  .hits button:hover {
    background: var(--surface);
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 12px;
  }
  .list > li {
    display: grid;
    gap: 10px;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
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
    grid-template-columns: repeat(8, minmax(0, 104px));
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
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .folder {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
  }
  .pick {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
    gap: 4px;
    max-height: 50vh;
    overflow-y: auto;
    margin: 12px 0;
  }
  .pick .s:disabled {
    cursor: default;
  }
  .pick .s.after {
    outline: 2px solid var(--fg);
    outline-offset: -2px;
  }
  .field {
    display: grid;
    gap: 4px;
    font-size: var(--t12);
    color: var(--muted);
  }
  p.muted {
    margin: 8px 0 0;
  }
</style>
