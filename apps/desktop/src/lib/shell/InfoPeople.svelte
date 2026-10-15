<script lang="ts">
  import { api } from '../ipc/api';
  import type { FaceBox } from '../ipc/bindings/FaceBox';
  import type { Person } from '../ipc/bindings/Person';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import FaceCrop from '../ui/FaceCrop.svelte';

  let { id }: { id: number } = $props();

  let faces = $state<FaceBox[]>([]);
  let people = $state<Person[]>([]);
  let asking = $state<number | null>(null);
  let typed = $state('');
  let seq = 0;

  $effect(() => {
    const target = id;
    void app.revision;
    const my = ++seq;
    void Promise.all([api.facesIn(target), api.people()])
      .then(([f, p]) => {
        if (my !== seq) return;
        faces = f;
        people = p;
      })
      .catch(() => {});
  });

  const nameOf = (f: FaceBox) => people.find((p) => p.id === f.person)?.name ?? null;
  const named = $derived(people.filter((p) => p.name));

  async function save(face: FaceBox) {
    const name = typed.trim();
    asking = null;
    typed = '';
    if (!name) return;
    const existing = named.find((p) => p.name!.toLowerCase() === name.toLowerCase());
    try {
      await api.assignFaces([face.id], existing?.id ?? null, existing ? null : name);
      toast.show(`This is ${existing?.name ?? name}.`);
    } catch (e) {
      toast.error(e);
    }
  }
</script>

{#if faces.length}
  <section data-testid="info-people">
    <h3>People</h3>
    <ul>
      {#each faces as f (f.id)}
        {@const name = nameOf(f)}
        <li>
          <FaceCrop face={f} size={32} />
          {#if asking === f.id}
            <form
              onsubmit={(e) => {
                e.preventDefault();
                void save(f);
              }}
            >
              <!-- svelte-ignore a11y_autofocus -->
              <input bind:value={typed} list="people-names" placeholder="Who is this?" aria-label="Who is this?" autofocus onblur={() => (asking = null)} />
            </form>
          {:else if name}
            <button
              class="link"
              onclick={() => f.person != null && app.go('timeline', { key: `person:${f.person}`, label: name, query: { people: [f.person] } })}>{name}</button
            >
          {:else}
            <button class="link muted" onclick={() => (asking = f.id)} data-testid="who-is-this">Who is this?</button>
          {/if}
        </li>
      {/each}
    </ul>
    <datalist id="people-names">
      {#each named as p (p.id)}<option value={p.name}></option>{/each}
    </datalist>
  </section>
{/if}

<style>
  section {
    padding: 14px 0;
    border-bottom: 1px solid var(--border);
  }
  h3 {
    margin: 0 0 6px;
    color: var(--muted);
    font-size: var(--t12);
    font-weight: 500;
  }
  ul {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  form {
    flex: 1;
  }
  input {
    width: 100%;
    height: 26px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    font-size: var(--t12);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .link.muted {
    color: var(--muted);
  }
</style>
