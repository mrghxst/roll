<script lang="ts">
  import { Download, Eye, EyeOff, Merge, Pencil, UserX } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { FaceBox } from '../ipc/bindings/FaceBox';
  import type { Person } from '../ipc/bindings/Person';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import FaceCrop from '../ui/FaceCrop.svelte';
  import Input from '../ui/Input.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { bytes, n } from '../util/format';

  let people = $state<Person[]>([]);
  let loaded = $state(false);
  let showHidden = $state(false);
  let picked = $state<number[]>([]);
  let naming = $state<{ id: number; name: string } | null>(null);
  /** The faces sheet of one person: "not this person" on any of them. */
  let open = $state<{ person: Person; faces: FaceBox[]; picked: number[] } | null>(null);

  const pack = $derived(app.pack('faces'));
  const installed = $derived(pack?.state === 'installed');

  $effect(() => {
    void app.revision;
    void app.historyRevision;
    void api
      .people()
      .then((p) => {
        people = p;
        loaded = true;
      })
      .catch((e) => toast.error(e));
  });

  const shown = $derived(people.filter((p) => showHidden || !p.hidden));
  const hiddenCount = $derived(people.filter((p) => p.hidden).length);
  const label = (p: Person) => p.name ?? 'No name yet';

  async function run(f: () => Promise<unknown>, done?: string) {
    try {
      await f();
      if (done) toast.show(done);
    } catch (e) {
      toast.error(e);
    }
  }

  function photos(p: Person) {
    app.go('timeline', { key: `person:${p.id}`, label: p.name ?? 'A person without a name', query: { people: [p.id] } });
  }

  async function saveName() {
    if (!naming) return;
    const { id, name } = naming;
    naming = null;
    await run(() => api.namePerson(id, name.trim() || null), name.trim() ? `Named ${name.trim()}. The name goes into the files at the next metadata write.` : undefined);
  }

  function toggle(id: number) {
    picked = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
  }

  async function merge() {
    // The named one (or the one with most faces) keeps its place.
    const chosen = people.filter((p) => picked.includes(p.id));
    const into = (chosen.find((p) => p.name) ?? chosen[0])!;
    const others = chosen.filter((p) => p.id !== into.id).map((p) => p.id);
    picked = [];
    await run(() => api.mergePeople(into.id, others), `Merged ${n(chosen.length)} people into ${label(into)}.`);
  }

  async function showFaces(p: Person) {
    try {
      open = { person: p, faces: await api.personFaces(p.id), picked: [] };
    } catch (e) {
      toast.error(e);
    }
  }

  async function notThem() {
    if (!open?.picked.length) return;
    const ids = open.picked;
    const who = label(open.person);
    await run(() => api.notPerson(ids), `${n(ids.length)} ${ids.length === 1 ? 'face is' : 'faces are'} no longer ${who}.`);
    open = { ...open, faces: open.faces.filter((f) => !ids.includes(f.id)), picked: [] };
  }
</script>

<div class="people">
  <header>
    <h1>People</h1>
    <p class="lead">
      Faces Roll found in your photos, grouped by who they look like. Give someone a name and it goes into the photos' files as a face region, which Immich and
      other apps read. Faces never leave this computer.
    </p>
  </header>

  {#if !installed}
    <EmptyState text={pack?.state === 'downloading' ? `Downloading the face models (${bytes(pack.done)} of ${bytes(pack.size)})…` : 'Finding people needs the face models (YuNet and SFace, about 39 MB). They run on this computer.'}>
      {#snippet action()}
        {#if pack && pack.state !== 'downloading' && pack.state !== 'installing'}
          <Button variant="primary" onclick={() => void run(() => api.downloadPack('faces'))} data-testid="download-faces">
            <Download size={14} strokeWidth={1.5} /> Download face models
          </Button>
        {/if}
      {/snippet}
    </EmptyState>
  {:else if loaded && !people.length}
    <EmptyState text="No people yet. Roll looks for faces after thumbnails are made; a person shows up once three photos of them are found." />
  {:else}
    <div class="bar">
      {#if picked.length >= 2}
        <Button variant="primary" onclick={() => void merge()} data-testid="merge-people"><Merge size={14} strokeWidth={1.5} /> Merge {n(picked.length)} people</Button>
        <Button variant="ghost" onclick={() => (picked = [])}>Clear</Button>
      {:else}
        <span class="muted">{n(shown.length)} {shown.length === 1 ? 'person' : 'people'}. Tick two or more to merge them.</span>
      {/if}
      <span class="spacer"></span>
      {#if hiddenCount}
        <Button variant="ghost" onclick={() => (showHidden = !showHidden)}>
          {showHidden ? 'Hide hidden people' : `Show ${n(hiddenCount)} hidden`}
        </Button>
      {/if}
    </div>
    <ul class="grid" data-testid="people">
      {#each shown as p (p.id)}
        <li class:hidden={p.hidden} class:picked={picked.includes(p.id)}>
          <button class="cover" onclick={() => photos(p)} aria-label="Photos of {label(p)}">
            {#if p.cover}<FaceCrop face={p.cover} size={112} />{/if}
          </button>
          {#if naming?.id === p.id}
            <form
              onsubmit={(e) => {
                e.preventDefault();
                void saveName();
              }}
            >
              <Input bind:value={naming.name} placeholder="Name" aria-label="Name" data-testid="person-name" autofocus />
            </form>
          {:else}
            <button class="name" class:unnamed={!p.name} onclick={() => (naming = { id: p.id, name: p.name ?? '' })} data-testid="name-person">
              {p.name ?? 'Add a name'}
              <Pencil size={12} strokeWidth={1.5} />
            </button>
          {/if}
          <span class="muted">{n(p.items)} {p.items === 1 ? 'photo' : 'photos'}</span>
          <div class="actions">
            <label class="tick" title="Pick to merge">
              <input type="checkbox" checked={picked.includes(p.id)} onchange={() => toggle(p.id)} aria-label="Pick {label(p)} to merge" />
            </label>
            <button class="icon" title="Faces of {label(p)}" aria-label="Faces of {label(p)}" onclick={() => void showFaces(p)} data-testid="person-faces">
              <UserX size={14} strokeWidth={1.5} />
            </button>
            <button
              class="icon"
              title={p.hidden ? 'Show again' : 'Hide this person'}
              aria-label={p.hidden ? `Show ${label(p)} again` : `Hide ${label(p)}`}
              onclick={() => void run(() => api.hidePerson(p.id, !p.hidden))}
            >
              {#if p.hidden}<Eye size={14} strokeWidth={1.5} />{:else}<EyeOff size={14} strokeWidth={1.5} />{/if}
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<Sheet open={open != null} title={open ? `Faces of ${label(open.person)}` : 'Faces'} width={620} onclose={() => (open = null)}>
  {#if open}
    <p class="lead">Pick the faces that are someone else. They leave {label(open.person)} and are not grouped with them again.</p>
    <div class="faces" data-testid="faces-grid">
      {#each open.faces as f (f.id)}
        <button
          class="face"
          class:out={open.picked.includes(f.id)}
          aria-pressed={open.picked.includes(f.id)}
          aria-label="Face {f.id}"
          onclick={() => {
            if (!open) return;
            open.picked = open.picked.includes(f.id) ? open.picked.filter((x) => x !== f.id) : [...open.picked, f.id];
          }}
        >
          <FaceCrop face={f} size={80} round={false} />
        </button>
      {/each}
    </div>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = null)}>Done</Button>
    <Button variant="primary" disabled={!open?.picked.length} onclick={() => void notThem()} data-testid="not-person">
      Not {open ? label(open.person) : ''}{open?.picked.length ? ` (${n(open.picked.length)})` : ''}
    </Button>
  {/snippet}
</Sheet>

<style>
  .people {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    max-width: 72ch;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    margin: 8px 0 16px;
  }
  .spacer {
    flex: 1;
  }
  .muted {
    color: var(--muted);
    font-size: var(--t12);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 16px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .grid li {
    display: grid;
    justify-items: center;
    gap: 4px;
    padding: 12px 8px 8px;
    border: 1px solid transparent;
    border-radius: var(--r-sheet);
  }
  .grid li.picked {
    border-color: var(--fg);
  }
  .grid li.hidden {
    opacity: 0.5;
  }
  .cover {
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  .cover:focus-visible {
    outline: 2px solid var(--fg);
    outline-offset: 2px;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 4px;
    border: 0;
    border-radius: var(--r-1);
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: var(--t13);
    font-weight: 500;
    cursor: pointer;
  }
  .name.unnamed {
    color: var(--muted);
    font-weight: 400;
  }
  .name:hover {
    background: var(--surface);
  }
  form {
    width: 100%;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .tick {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
  }
  .tick input {
    accent-color: var(--fg);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: var(--r-1);
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--surface);
    color: var(--fg);
  }
  .faces {
    display: grid;
    grid-template-columns: repeat(auto-fill, 80px);
    gap: 6px;
    max-height: 60vh;
    overflow: auto;
  }
  .face {
    padding: 0;
    border: 2px solid transparent;
    border-radius: var(--r-1);
    background: none;
    cursor: pointer;
    line-height: 0;
  }
  .face.out {
    border-color: var(--danger);
    opacity: 0.6;
  }
</style>
