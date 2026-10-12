<script lang="ts">
  import { Cloud, FileCode, RefreshCw, Settings as Gear } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { AlbumInfo } from '../ipc/bindings/AlbumInfo';
  import type { ImmichPlan } from '../ipc/bindings/ImmichPlan';
  import type { ImmichStatus } from '../ipc/bindings/ImmichStatus';
  import type { ImmichUser } from '../ipc/bindings/ImmichUser';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Switch from '../ui/Switch.svelte';
  import { bytes, n, plural } from '../util/format';

  let status = $state<ImmichStatus | null>(null);
  let plan = $state<ImmichPlan | null>(null);
  let albums = $state<AlbumInfo[]>([]);
  let users = $state<ImmichUser[]>([]);
  let remove = $state(false);
  let checking = $state(false);

  const connected = $derived(!!status?.server);
  const syncing = $derived(app.activeJobs.some((j) => j.kind === 'immich'));

  async function loadStatus() {
    checking = true;
    try {
      status = await api.immichStatus();
      users = status.server ? await api.immichUsers() : [];
    } catch (e) {
      toast.error(e);
    } finally {
      checking = false;
    }
  }

  $effect(() => {
    void app.settings?.network.immich;
    void app.settings?.immich_url;
    void loadStatus();
  });

  $effect(() => {
    void app.revision;
    void app.historyRevision;
    void Promise.all([api.immichPlan(), api.immichAlbums()])
      .then(([p, a]) => {
        plan = p;
        albums = a;
      })
      .catch((e) => toast.error(e));
  });

  const nothing = $derived(plan != null && plan.uploads + plan.updates + plan.new_albums + plan.album_updates + (remove ? plan.removes : 0) === 0);

  async function sync() {
    try {
      await api.immichSync(remove);
      toast.show('Syncing with Immich. Progress shows in Activity.');
      remove = false;
    } catch (e) {
      toast.error(e);
    }
  }

  async function setAlbum(a: AlbumInfo, send: boolean, share: string[]) {
    try {
      await api.immichAlbum(a.event_id, send, share);
      a.send = send;
      a.share = share;
      plan = await api.immichPlan();
    } catch (e) {
      toast.error(e);
    }
  }

  async function saveScript() {
    try {
      if (await api.saveImmichScript()) toast.show('Saved the Immich CLI script');
    } catch (e) {
      toast.error(e);
    }
  }

  const goSettings = () => app.go('settings');
</script>

<div class="immich">
  <header>
    <h1>Immich</h1>
    <p class="lead">
      Immich gets the part of the archive you choose: Photos by default, confirmed events as albums, and anything you mark Send to Immich.
      Every sync shows its plan first and never changes the files here.
    </p>
  </header>

  <section class="card" data-testid="immich-status">
    {#if !status}
      <p class="muted">Checking…</p>
    {:else if !status.url || !status.has_key}
      <p>Add your server address and an API key in Settings › Immich.</p>
      <div class="row"><Button onclick={goSettings}><Gear size={14} strokeWidth={1.5} /> Open Settings</Button></div>
    {:else if !status.on}
      <p>Immich is off in Settings › Network. Turning it on lets Roll contact <span class="mono">{status.url}</span>.</p>
      <div class="row">
        <Button variant="primary" onclick={() => void app.saveSettings({ network: { ...app.settings!.network, immich: true } })} data-testid="immich-on"
          >Turn on Immich</Button
        >
      </div>
    {:else if status.error}
      <p class="warn">{status.error}</p>
      <div class="row">
        <Button disabled={checking} onclick={() => void loadStatus()}><RefreshCw size={14} strokeWidth={1.5} /> Try again</Button>
        <Button variant="ghost" onclick={goSettings}>Settings</Button>
      </div>
    {:else}
      <p><Cloud size={14} strokeWidth={1.5} /> Connected to Immich {status.server} at <span class="mono">{status.url}</span> as {status.user}.</p>
    {/if}
  </section>

  {#if plan}
    <section class="card" data-testid="immich-plan">
      <h2>Next sync</h2>
      <ul class="facts">
        <li><strong>{n(plan.uploads)}</strong> to send{plan.uploads ? `, ${bytes(plan.upload_bytes)}` : ''}</li>
        <li><strong>{n(plan.updates)}</strong> to update (dates, places, tags, ratings changed here)</li>
        <li><strong>{n(plan.new_albums)}</strong> new {plan.new_albums === 1 ? 'album' : 'albums'}{plan.album_updates ? `, ${n(plan.album_updates)} to fill` : ''}</li>
        <li class="muted">{plural(plan.in_sync, 'item')} already up to date</li>
      </ul>
      {#if plan.upload_samples.length}
        <div class="samples">
          {#each plan.upload_samples as id (id)}<img src={api.thumbUrl(id, 1)} alt="" loading="lazy" />{/each}
        </div>
      {/if}
      {#if plan.removes}
        <div class="note">
          <p>
            {plural(plan.removes, 'item')}
            {plan.removes === 1 ? 'is' : 'are'} in Immich but now marked Keep local only, or in Roll's trash.
          </p>
          <div class="row">
            <Switch label="Move them to Immich's trash" bind:checked={remove} />
            <span>Move them to Immich's trash (Immich keeps its trash for 30 days; the files here stay)</span>
          </div>
        </div>
      {/if}
      <div class="row end">
        <Button variant="primary" disabled={!connected || syncing || nothing} onclick={() => void sync()} data-testid="immich-sync">
          <RefreshCw size={14} strokeWidth={1.5} />
          {syncing ? 'Syncing…' : 'Sync now'}
        </Button>
      </div>
    </section>
  {/if}

  <section>
    <h2>Event albums</h2>
    {#if !albums.length}
      <p class="muted">Confirmed events become albums. Confirm events in Events.</p>
    {:else}
      <ul class="albums" data-testid="immich-albums">
        {#each albums as a (a.event_id)}
          <li>
            <Switch label="Album in Immich for {a.name}" checked={a.send} onchange={(v) => void setAlbum(a, v, a.share)} />
            <div class="name">
              <strong>{a.name}</strong>
              <span class="muted">{plural(a.items, 'item')} · {a.album_id ? 'in Immich' : a.send ? 'created at the next sync' : 'stays local'}</span>
            </div>
            {#if a.send && users.length}
              <div class="share" role="group" aria-label="Share {a.name} with">
                {#each users as u (u.id)}
                  <button
                    class="chip"
                    aria-pressed={a.share.includes(u.id)}
                    onclick={() => void setAlbum(a, a.send, a.share.includes(u.id) ? a.share.filter((x) => x !== u.id) : [...a.share, u.id])}
                    title={u.email}>{u.name}</button
                  >
                {/each}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if users.length}<p class="muted">Shared albums let the people you pick see the album in Immich; they can't change it.</p>{/if}
    {/if}
  </section>

  <section>
    <h2>Without the API</h2>
    <p class="muted">
      A script for the Immich command-line tool sends the same folders, with their sidecars, and makes the event albums. It sends whole
      folders, so items marked Keep local only in them go too. The API key is not written into it.
    </p>
    <div class="row"><Button onclick={() => void saveScript()}><FileCode size={14} strokeWidth={1.5} /> Save a script…</Button></div>
    <p class="muted">
      Leave the Immich app's automatic phone backup off: it would send the whole camera roll, not your selection. New photos reach the
      archive through your usual export and the Inbox.
    </p>
  </section>
</div>

<style>
  .immich {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  .immich > * {
    max-width: 880px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  h2 {
    margin: 20px 0 8px;
    font-size: var(--t14);
    font-weight: 600;
  }
  .card h2 {
    margin-top: 0;
  }
  .lead {
    margin: 0 0 16px;
    color: var(--muted);
    max-width: 72ch;
  }
  .card {
    display: grid;
    gap: 10px;
    margin-bottom: 12px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    font-size: var(--t13);
  }
  .card p {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .warn {
    color: var(--danger, var(--fg));
  }
  .facts {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
  }
  .samples {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .samples img {
    width: 56px;
    height: 56px;
    object-fit: cover;
    border-radius: var(--r-1);
    background: var(--surface);
  }
  .note {
    display: grid;
    gap: 8px;
    padding: 10px 12px;
    border-radius: var(--r-btn);
    background: var(--surface);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: var(--t13);
  }
  .row.end {
    justify-content: flex-end;
  }
  .muted {
    color: var(--muted);
    font-size: var(--t12);
  }
  section > p {
    margin: 0 0 8px;
  }
  .albums {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }
  .albums li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .name {
    display: grid;
    flex: 1;
  }
  .name strong {
    font-weight: 500;
  }
  .share {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg);
    color: var(--muted);
    font-size: var(--t12);
    cursor: pointer;
  }
  .chip[aria-pressed='true'] {
    background: var(--fg);
    color: var(--bg);
    border-color: var(--fg);
  }
</style>
