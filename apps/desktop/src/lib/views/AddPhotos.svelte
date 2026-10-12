<script lang="ts">
  import { ArrowLeft, ArrowRight, CalendarClock, Check, FolderInput, MapPin, Undo2, X } from '@lucide/svelte';
  import { untrack } from 'svelte';
  import { api, emptyQuery } from '../ipc/api';
  import type { ImportInfo } from '../ipc/bindings/ImportInfo';
  import type { ImportOptions } from '../ipc/bindings/ImportOptions';
  import type { ImportPlan } from '../ipc/bindings/ImportPlan';
  import type { PlaceGroup } from '../ipc/bindings/PlaceGroup';
  import type { Timeline } from '../ipc/timeline';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Switch from '../ui/Switch.svelte';
  import { accuracy, bytes, n, plural, shortDate, time } from '../util/format';

  type Step = 'source' | 'dates' | 'places' | 'event' | 'add';

  let list = $state<ImportInfo[]>([]);
  let loaded = $state(false);
  let step = $state<Step>('source');
  let shownFor = $state<number | null>(null);
  let staged = $state<Timeline | null>(null);
  let archiveDays = $state<Map<number, { ids: number[]; versions: number[] }>>(new Map());
  let groups = $state<PlaceGroup[]>([]);
  let unplaced = $state<number[]>([]);
  let plan = $state<ImportPlan | null>(null);
  let planError = $state<string | null>(null);
  let picked = $state<number[]>([]);
  let nameDraft = $state('');
  let opts = $state<ImportOptions>({ mode: 'copy', skip_known: true, root_id: null });
  let busy = $state(false);

  const info = $derived(list.find((i) => i.id === app.importing) ?? null);
  const known = $derived(new Set([...(info?.exact ?? []), ...(info?.similar ?? [])]));
  const leftOut = $derived(new Set(info?.left_out ?? []));
  const applying = $derived(app.activeJobs.some((j) => j.kind === 'apply'));

  $effect(() => {
    void app.revision;
    void app.importsRevision;
    void app.thumbsRevision;
    void api
      .imports()
      .then((l) => {
        list = l;
        loaded = true;
        if (!l.some((i) => i.id === app.importing)) app.importing = l[0]?.id ?? null;
      })
      .catch((e) => toast.error(e));
  });

  // A different import: start at its first step (or the last when done).
  // The name field follows the proposal until the owner types their own.
  let proposed = '';
  $effect(() => {
    const i = info;
    if (!i) return;
    if (i.id !== shownFor) {
      shownFor = i.id;
      step = i.state === 'applied' ? 'add' : 'source';
      picked = [];
      nameDraft = i.name;
    } else if (!i.custom_name && untrack(() => nameDraft) === proposed) {
      nameDraft = i.name;
    }
    proposed = i.name;
  });

  // The items, by day, next to what the library has from the same days.
  $effect(() => {
    const i = info;
    void app.revision;
    void app.thumbsRevision;
    if (!i) return;
    void (async () => {
      const t = await api.timeline({ ...emptyQuery(), staged: i.root_id });
      staged = t;
      if (!i.from_local) {
        archiveDays = new Map();
        return;
      }
      const to = new Date(Date.parse(`${i.to_local.slice(0, 10)}T00:00:00Z`) + 86_400_000).toISOString().slice(0, 19);
      const a = await api.timeline({ ...emptyQuery(), from_local: `${i.from_local.slice(0, 10)}T00:00:00`, to_local: to });
      const m = new Map<number, { ids: number[]; versions: number[] }>();
      for (let k = a.count - 1; k >= 0; k--) {
        const d = a.days[k]!;
        const e = m.get(d) ?? { ids: [], versions: [] };
        e.ids.push(a.ids[k]!);
        e.versions.push(a.versions[k]!);
        m.set(d, e);
      }
      archiveDays = m;
    })().catch((e) => toast.error(e));
  });

  const stagedDays = $derived.by(() => {
    const m = new Map<number, { ids: number[]; versions: number[] }>();
    const t = staged;
    if (!t) return m;
    for (let k = t.count - 1; k >= 0; k--) {
      const d = t.days[k]!;
      const e = m.get(d) ?? { ids: [], versions: [] };
      e.ids.push(t.ids[k]!);
      e.versions.push(t.versions[k]!);
      m.set(d, e);
    }
    return m;
  });
  const version = $derived.by(() => {
    const m = new Map<number, number>();
    const t = staged;
    if (t) for (let k = 0; k < t.count; k++) m.set(t.ids[k]!, t.versions[k]!);
    return m;
  });
  const thumb = (id: number) => api.thumbUrl(id, (version.get(id) ?? 1) + app.thumbsRevision);

  // Place suggestions and the items still without a place.
  $effect(() => {
    const i = info;
    void app.revision;
    if (!i || step !== 'places') return;
    void Promise.all([api.importPlaces(i.id), api.queryIds({ ...emptyQuery(), staged: i.root_id, location: 'none' })])
      .then(([g, ids]) => {
        groups = g;
        const suggested = new Set(g.flatMap((x) => x.ids));
        unplaced = ids.filter((x) => !suggested.has(x));
      })
      .catch((e) => toast.error(e));
  });

  // The plan, whenever the options change.
  $effect(() => {
    const i = info;
    const o = { ...opts };
    void app.revision;
    if (!i || step !== 'add' || i.state !== 'checking') return;
    void api
      .importPlan(i.id, o)
      .then((p) => {
        plan = p;
        planError = null;
      })
      .catch((e) => {
        plan = null;
        planError = String(e).replace(/^Error: /, '');
      });
  });

  async function run(f: () => Promise<unknown>, done?: string) {
    busy = true;
    try {
      await f();
      if (done) toast.show(done);
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  function dayLabel(d: number) {
    const s = String(d);
    return shortDate(`${s.slice(0, 4)}-${s.slice(4, 6)}-${s.slice(6, 8)}T12:00:00`);
  }

  function range(i: ImportInfo) {
    if (!i.from_local) return 'no dates';
    const a = shortDate(i.from_local);
    const b = shortDate(i.to_local);
    return a === b ? a : `${a} – ${b}`;
  }

  function togglePick(id: number) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  async function applySmart(i: ImportInfo) {
    const s = i.smart!;
    await run(async () => {
      await api.dateApply({
        ids: i.ids.filter((x) => !leftOut.has(x)),
        mode: 'smart',
        reference: null,
        second: null,
        offset_min: s.offset_min,
        amount: { years: 0, days: 0, hours: 0, minutes: 0, seconds: s.seconds },
        exact: null,
        camera_from: null,
        camera_to: null,
      });
      await api.importMark(i.id, true, null);
    }, `Shifted ${plural(i.count, 'item')} by ${s.label}`);
  }

  async function acceptGroup(g: PlaceGroup) {
    await run(async () => {
      const r = await api.acceptPlaces(g.key);
      if (r.zones.length) app.zones = r.zones;
    }, `Placed ${plural(g.count, 'item')} at ${g.place}`);
  }

  const unplacedDays = $derived.by(() => {
    const set = new Set(unplaced);
    const out: { day: number; ids: number[] }[] = [];
    for (const [day, e] of stagedDays) {
      const ids = e.ids.filter((x) => set.has(x));
      if (ids.length) out.push({ day, ids });
    }
    return out.sort((a, b) => a.day - b.day);
  });

  async function saveName(i: ImportInfo) {
    const v = nameDraft.trim();
    if (v === i.name) return;
    await run(() => api.importName(i.id, v || null));
  }

  async function apply(i: ImportInfo) {
    await run(async () => {
      await saveName(i);
      await api.importApply(i.id, { ...opts });
    }, `${opts.mode === 'move' ? 'Moving' : 'Copying'} ${plural(plan?.files ?? 0, 'item')} into ${i.name}`);
  }

  function showEvent(i: ImportInfo) {
    const tag = `Event/${i.name.replaceAll('/', '-')}`;
    app.go('timeline', { key: `tag:${tag}`, label: i.name, query: { tags: [tag] } });
  }

  const STEPS: { key: Step; label: string }[] = [
    { key: 'source', label: 'Source' },
    { key: 'dates', label: 'Dates' },
    { key: 'places', label: 'Places' },
    { key: 'event', label: 'Event' },
    { key: 'add', label: 'Add' },
  ];
  const stepIndex = $derived(STEPS.findIndex((s) => s.key === step));
  const doneOf = (i: ImportInfo, s: Step) => (s === 'dates' ? i.dates_ok : s === 'places' ? i.places_ok : s === 'add' ? i.state === 'applied' : false);
</script>

{#snippet strip(ids: number[], limit: number, label: string)}
  <div class="strip" role="group" aria-label={label}>
    {#each ids.slice(0, limit) as id, k (id)}
      <button class="s" ondblclick={() => viewer.open(Uint32Array.from(ids), k, null)} aria-label="Open"><img src={thumb(id)} alt="" loading="lazy" /></button>
    {/each}
    {#if ids.length > limit}<span class="more muted">+{n(ids.length - limit)}</span>{/if}
  </div>
{/snippet}

<div class="add">
  <header>
    <div class="title">
      <h1>Add photos</h1>
      <Button onclick={() => void run(() => app.addPhotos())} data-testid="add-folder"><FolderInput size={14} strokeWidth={1.5} /> Choose a folder…</Button>
    </div>
    <p class="lead">
      Bring a folder from outside your library in, usually as one event: a camera card, a friend's photos, an old trip folder. Roll reads it
      without changing it, checks dates and places against your library, then copies it in. You can also drop a folder onto the window.
    </p>
    {#if list.length > 1}
      <div class="tabs" role="tablist" aria-label="Open imports">
        {#each list as i (i.id)}
          <button role="tab" aria-selected={i.id === app.importing} onclick={() => (app.importing = i.id)}>{i.name}</button>
        {/each}
      </div>
    {/if}
  </header>

  {#if loaded && !info}
    <p class="empty muted">Nothing is being added. Choose a folder to start.</p>
  {:else if info}
    {@const i = info}
    <nav class="steps" aria-label="Steps">
      {#each STEPS as s, k (s.key)}
        <button class="stepbtn" aria-current={step === s.key ? 'step' : undefined} onclick={() => (step = s.key)} data-testid="step-{s.key}">
          <span class="num" class:done={doneOf(i, s.key)}>{#if doneOf(i, s.key)}<Check size={12} strokeWidth={2} />{:else}{k + 1}{/if}</span>
          {s.label}
        </button>
      {/each}
    </nav>

    <section class="body" data-testid="import-{step}">
      {#if step === 'source'}
        <dl class="facts">
          <dt>Folder</dt>
          <dd class="mono">{i.source}</dd>
          <dt>Items</dt>
          <dd>{plural(i.photos, 'photo')}{i.videos ? `, ${plural(i.videos, 'video')}` : ''}</dd>
          <dt>Dates</dt>
          <dd>{range(i)}</dd>
          <dt>Cameras</dt>
          <dd>{i.cameras.map((c) => `${c.label} (${n(c.count)})`).join(', ')}</dd>
          <dt>Formats</dt>
          <dd>{i.formats.map((c) => `${c.label} (${n(c.count)})`).join(', ')}</dd>
        </dl>
        {#if i.pending}<p class="muted">Making thumbnails: {n(i.pending)} left.</p>{/if}
        {#if known.size}
          <div class="note" data-testid="known">
            <p>
              {plural(known.size, 'item')}
              {known.size === 1 ? 'is' : 'are'} already in your library{i.exact.length && i.similar.length
                ? ` (${n(i.exact.length)} exact ${i.exact.length === 1 ? 'copy' : 'copies'}, ${n(i.similar.length)} looking the same)`
                : i.exact.length
                  ? ' as exact copies'
                  : ', or look the same as photos you have'}.
            </p>
            <Switch label="Leave out items already in the library" bind:checked={opts.skip_known} />
            <span class="muted">Leave out items already in the library</span>
          </div>
        {/if}
        <div class="pickbar">
          <span class="muted">Click items to pick them, double-click to open.</span>
          {#if picked.length}
            {@const out = picked.filter((x) => leftOut.has(x))}
            {@const inn = picked.filter((x) => !leftOut.has(x))}
            {#if inn.length}<Button onclick={() => void run(async () => { await api.importLeaveOut(i.id, inn, true); picked = []; })}>Leave out {n(inn.length)}</Button>{/if}
            {#if out.length}<Button onclick={() => void run(async () => { await api.importLeaveOut(i.id, out, false); picked = []; })}>Bring back {n(out.length)}</Button>{/if}
            <Button variant="ghost" onclick={() => (picked = [])}>Clear</Button>
          {/if}
        </div>
        <div class="grid" data-testid="staged-items">
          {#each i.ids as id, k (id)}
            <button
              class="s"
              class:picked={picked.includes(id)}
              class:out={leftOut.has(id) || (opts.skip_known && known.has(id))}
              onclick={() => togglePick(id)}
              ondblclick={() => viewer.open(Uint32Array.from(i.ids), k, null)}
              aria-pressed={picked.includes(id)}
              aria-label="Item {k + 1}"
            >
              <img src={thumb(id)} alt="" loading="lazy" />
              {#if leftOut.has(id)}<span class="badge">Left out</span>{:else if known.has(id)}<span class="badge">In library</span>{/if}
            </button>
          {/each}
        </div>
      {:else if step === 'dates'}
        {#each i.date_sets.filter((s) => s.reasons.length) as s (s.key)}
          <div class="note warn">
            <strong>{s.label}, {plural(s.count, 'item')}</strong>
            {#each s.reasons as r (r)}<p>{r}</p>{/each}
          </div>
        {/each}
        {#if i.no_date}<div class="note warn"><p>{plural(i.no_date, 'item')} {i.no_date === 1 ? 'has' : 'have'} no date taken; Roll guessed from names, folders or file times.</p></div>{/if}
        {#if i.smart && Math.abs(i.smart.seconds) >= 120}
          {@const s = i.smart}
          <div class="card" data-testid="smart-offset">
            <p>
              Phone photos of the same moments suggest the camera clock is off by <strong>{s.label}</strong>{s.offset_min != null
                ? `, in time zone UTC${s.offset_min >= 0 ? '+' : '−'}${Math.abs(s.offset_min / 60)}`
                : ''}. {n(s.matched)} of {n(s.compared)} photos have a look-alike phone photo at that offset.
            </p>
            <div class="pairs">
              {#each s.pairs as p (p.item)}
                <div class="pair">
                  <img src={thumb(p.item)} alt="" />
                  <img src={api.thumbUrl(p.phone, 1)} alt="" />
                  <span class="muted">{time(p.item_local)} here, {time(p.phone_local)} on the phone</span>
                </div>
              {/each}
            </div>
            <Button variant="primary" disabled={busy} onclick={() => void applySmart(i)} data-testid="apply-smart">
              <CalendarClock size={14} strokeWidth={1.5} /> Shift by {s.label}
            </Button>
          </div>
        {:else if i.smart}
          <p class="ok"><Check size={14} strokeWidth={1.5} /> Phone photos of the same moments agree with these dates ({n(i.smart.matched)} matched).</p>
        {:else}
          <p class="muted">No phone photos in your library look like these, so Roll cannot check the clock against them. Compare the days below.</p>
        {/if}
        <h2>By day, next to your library</h2>
        <ul class="days">
          {#each [...stagedDays.keys()].sort((a, b) => a - b) as d (d)}
            {@const here = stagedDays.get(d)!}
            {@const lib = archiveDays.get(d)}
            <li>
              <span class="day">{dayLabel(d)}</span>
              <div>
                <span class="muted">Adding {n(here.ids.length)}</span>
                {@render strip(here.ids, 12, `Adding on ${dayLabel(d)}`)}
              </div>
              <div>
                <span class="muted">{lib ? `In your library: ${n(lib.ids.length)}` : 'Nothing in your library on this day'}</span>
                {#if lib}{@render strip(lib.ids, 12, `Library on ${dayLabel(d)}`)}{/if}
              </div>
            </li>
          {/each}
        </ul>
        <div class="foot">
          <Button onclick={() => (app.dating = i.ids.filter((x) => !leftOut.has(x)))}><CalendarClock size={14} strokeWidth={1.5} /> Change dates…</Button>
          <Button
            variant={i.dates_ok ? 'secondary' : 'primary'}
            onclick={() =>
              void run(async () => {
                await api.importMark(i.id, true, null);
                step = 'places';
              })}
            data-testid="dates-ok"><Check size={14} strokeWidth={1.5} /> Dates look right</Button
          >
        </div>
      {:else if step === 'places'}
        <p>
          {#if i.no_place === 0}
            <Check size={14} strokeWidth={1.5} /> Every item has a place{i.place ? `: ${i.place}` : ''}.
          {:else}
            {plural(i.no_place, 'item')} without a place{i.count - i.no_place > 0 ? `, ${n(i.count - i.no_place)} placed${i.place ? ` in ${i.place}` : ''}` : ''}.
          {/if}
        </p>
        {#each groups as g (g.key)}
          <div class="card" data-testid="place-group">
            <p>
              <strong>{plural(g.count, 'item')}, {shortDate(g.from_local)}{shortDate(g.to_local) !== shortDate(g.from_local) ? ` – ${shortDate(g.to_local)}` : ''}: {g.place}</strong>
            </p>
            <p class="muted">From {g.evidence}, {accuracy(g.accuracy_m)}.</p>
            <div class="row">
              <Button variant="primary" disabled={busy} onclick={() => void acceptGroup(g)} data-testid="accept-place"><MapPin size={14} strokeWidth={1.5} /> Accept</Button>
              <Button variant="ghost" onclick={() => (app.locating = g.ids)}>Choose another place…</Button>
            </div>
          </div>
        {/each}
        {#if unplaced.length}
          <h2>Without a suggestion</h2>
          <div class="row">
            <Button onclick={() => (app.locating = unplaced)}><MapPin size={14} strokeWidth={1.5} /> One place for all {n(unplaced.length)}…</Button>
          </div>
          <ul class="days">
            {#each unplacedDays as d (d.day)}
              <li>
                <span class="day">{dayLabel(d.day)}</span>
                {@render strip(d.ids, 8, `Without a place on ${dayLabel(d.day)}`)}
                <Button variant="ghost" onclick={() => (app.locating = d.ids)}>Place for this day…</Button>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="foot">
          <Button
            variant={i.places_ok ? 'secondary' : 'primary'}
            onclick={() =>
              void run(async () => {
                await api.importMark(i.id, null, true);
                step = 'event';
              })}
            data-testid="places-ok"><Check size={14} strokeWidth={1.5} /> Places look right</Button
          >
        </div>
      {:else if step === 'event'}
        <label class="field">
          <span>Event name</span>
          <Input bind:value={nameDraft} aria-label="Event name" onblur={() => void saveName(i)} onkeydown={(e: KeyboardEvent) => e.key === 'Enter' && void saveName(i)} />
        </label>
        <p class="muted">
          {i.custom_name ? 'Your name.' : 'Proposed from the folder name, or from the dates and the place.'} The items get the tag
          <span class="mono">Event/{nameDraft.trim() || i.name}</span>.
        </p>
        <p>Sorted as: {i.categories.map((c) => `${c.label} ${n(c.count)}`).join(', ')}.</p>
        <p class="muted">
          Screenshots, memes and other non-photos go to their usual folders. Everything else goes to the event folder and counts as Photos.
        </p>
        {#if i.from_local && Date.parse(`${i.to_local}Z`) - Date.parse(`${i.from_local}Z`) > 14 * 86_400_000}
          <p class="muted">These items span more than two weeks. After adding them you can split the event in Events.</p>
        {/if}
      {:else if i.state === 'applied'}
        <div class="card" data-testid="import-done">
          <p><strong>{applying ? `Adding to ${i.name}…` : `Added to ${i.name}.`}</strong></p>
          <p class="muted">
            {applying ? 'Copies are checked against the originals as they are made.' : 'The batch is in History; undo takes the copies back out.'}
            {i.count ? `${plural(i.count, 'item')} stayed in the source.` : ''}
          </p>
          <div class="row">
            <Button variant="primary" disabled={applying} onclick={() => showEvent(i)}>Show them</Button>
            {#if i.batch_id != null}<Button disabled={applying} onclick={() => void run(() => api.undo(i.batch_id!), 'Taking the items back out')}><Undo2 size={14} strokeWidth={1.5} /> Undo</Button>{/if}
            <Button variant="ghost" disabled={applying} onclick={() => void run(() => api.importClose(i.id))} data-testid="close-import">Close</Button>
          </div>
        </div>
      {:else}
        <div class="opts">
          <Segmented
            label="Copy or move"
            options={[
              { value: 'copy', label: 'Copy' },
              { value: 'move', label: 'Move' },
            ]}
            bind:value={opts.mode}
          />
          <span class="muted">{opts.mode === 'copy' ? 'The source stays untouched; each copy is checked against its original.' : 'The files leave the source folder.'}</span>
        </div>
        {#if app.roots.length > 1}
          <label class="field">
            <span>Library folder</span>
            <select bind:value={opts.root_id} aria-label="Library folder">
              <option value={null}>{app.roots[0]?.path}</option>
              {#each app.roots.slice(1) as r (r.id)}<option value={r.id}>{r.path}</option>{/each}
            </select>
          </label>
        {/if}
        {#if !i.dates_ok || !i.places_ok}
          <p class="note warn">
            {!i.dates_ok ? 'Check the dates' : 'Check the places'} first: nothing is accepted without a look.
          </p>
        {/if}
        {#if planError}<p class="note warn">{planError}</p>{/if}
        {#if plan}
          <div class="card" data-testid="import-plan">
            <p>
              <strong>{plural(plan.files, 'file')}</strong>{plan.sidecars ? ` and ${plural(plan.sidecars, 'sidecar')}` : ''}, {bytes(plan.bytes)}, go to
              <span class="mono">{plan.root_path}</span>.
              {plan.skipped ? `${plural(plan.skipped, 'item')} stay${plan.skipped === 1 ? 's' : ''} in the source.` : ''}
            </p>
            <ul class="folders">
              {#each plan.folders as f (f.path)}
                <li><span class="mono">{f.path}</span> <span class="muted">{n(f.count)}{f.new ? ', new folder' : ''}</span></li>
              {/each}
            </ul>
            <p class="muted">
              {plan.writes
                ? 'Dates, places and tags are written into the new files and their sidecars.'
                : 'ExifTool was not found: the changes stay in Roll until you save them from Organize.'}
              {plan.free >= 0 ? `${bytes(plan.free)} free there.` : ''}
            </p>
          </div>
        {/if}
        <div class="foot">
          <Button variant="ghost" onclick={() => void run(() => api.importClose(i.id))}><X size={14} strokeWidth={1.5} /> Stop and forget this folder</Button>
          <Button variant="primary" disabled={busy || !plan || !i.dates_ok || !i.places_ok} onclick={() => void apply(i)} data-testid="apply-import">
            {opts.mode === 'move' ? 'Move' : 'Copy'} {plan ? plural(plan.files, 'item') : ''} into {i.name}
          </Button>
        </div>
      {/if}
    </section>

    {#if step !== 'add'}
      <div class="nav">
        <Button variant="ghost" disabled={stepIndex === 0} onclick={() => (step = STEPS[stepIndex - 1]!.key)}><ArrowLeft size={14} strokeWidth={1.5} /> Back</Button>
        <Button
          variant="ghost"
          onclick={() => {
            if (step === 'event' && info) void saveName(info);
            step = STEPS[stepIndex + 1]!.key;
          }}
          data-testid="next-step">Next <ArrowRight size={14} strokeWidth={1.5} /></Button
        >
      </div>
    {/if}
  {/if}
</div>

<style>
  .add {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  header,
  .steps,
  .body,
  .nav {
    max-width: 980px;
  }
  .title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
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
  .lead {
    margin: 0 0 16px;
    color: var(--muted);
    max-width: 72ch;
  }
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
  }
  .tabs button {
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--muted);
    font-size: var(--t13);
    cursor: pointer;
  }
  .tabs button[aria-selected='true'] {
    color: var(--fg);
    background: var(--surface);
  }
  .steps {
    display: flex;
    gap: 4px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 16px;
  }
  .stepbtn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px 6px 6px;
    border: 0;
    border-radius: var(--r-btn);
    background: none;
    color: var(--muted);
    font-size: var(--t13);
    cursor: pointer;
  }
  .stepbtn[aria-current='step'] {
    background: var(--surface);
    color: var(--fg);
  }
  .num {
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid var(--border);
    font-size: var(--t12);
  }
  .num.done {
    background: var(--fg);
    color: var(--bg);
    border-color: var(--fg);
  }
  .body {
    display: grid;
    gap: 12px;
    font-size: var(--t13);
  }
  .body p {
    margin: 0;
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 0;
  }
  .facts dt {
    color: var(--muted);
  }
  .facts dd {
    margin: 0;
  }
  .note,
  .card {
    display: grid;
    gap: 8px;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
  }
  .note {
    grid-template-columns: 1fr;
    background: var(--surface);
  }
  .card > :global(.btn) {
    justify-self: start;
  }
  .pairs {
    max-width: 760px;
  }
  .note:has(> :global(.switch)) {
    grid-template-columns: auto 1fr;
    align-items: center;
  }
  .note:has(> :global(.switch)) p {
    grid-column: 1 / -1;
  }
  .ok {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .muted {
    color: var(--muted);
    font-size: var(--t12);
  }
  .pickbar,
  .row,
  .opts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));
    gap: 4px;
  }
  .s {
    position: relative;
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
  .s.picked {
    outline: 2px solid var(--fg);
    outline-offset: -2px;
  }
  .s.out img {
    opacity: 0.35;
  }
  .badge {
    position: absolute;
    left: 4px;
    bottom: 4px;
    padding: 1px 5px;
    border-radius: var(--r-1);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t11, 11px);
  }
  .pairs {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 8px;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
  }
  .pair img {
    width: 100%;
    aspect-ratio: 4 / 3;
    object-fit: cover;
    border-radius: var(--r-1);
  }
  .pair span {
    grid-column: 1 / -1;
  }
  .days {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 10px;
  }
  .days li {
    display: grid;
    grid-template-columns: 110px 1fr 1fr;
    gap: 12px;
    align-items: start;
  }
  .day {
    font-weight: 500;
    padding-top: 2px;
  }
  .strip {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    margin-top: 4px;
  }
  .strip .s {
    width: 44px;
    height: 44px;
  }
  .more {
    align-self: center;
    padding-left: 4px;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 8px;
  }
  .nav {
    display: flex;
    justify-content: space-between;
    margin-top: 20px;
  }
  .field {
    display: grid;
    gap: 4px;
    max-width: 480px;
    font-size: var(--t12);
    color: var(--muted);
  }
  select {
    height: 32px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
  }
  .folders {
    margin: 0;
    padding-left: 18px;
  }
  .empty {
    margin-top: 24px;
  }
</style>
