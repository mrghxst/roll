<script lang="ts">
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { n } from '../util/format';

  const ids = $derived(app.teaching);
  const ready = $derived(app.pack('image-model')?.state === 'installed');

  let mode = $state<'existing' | 'new'>('new');
  let existing = $state('');
  let name = $state('');
  let parent = $state('');
  let definition = $state('');
  let busy = $state(false);

  const top = $derived(app.categories.filter((c) => c.parent_id === null && c.key !== 'unsorted'));
  const subs = (id: number) => app.categories.filter((c) => c.parent_id === id);

  $effect(() => {
    if (ids) {
      mode = 'new';
      existing = '';
      name = '';
      parent = '';
      definition = '';
      busy = false;
    }
  });

  const valid = $derived(mode === 'existing' ? !!existing : !!name.trim());

  async function submit() {
    const target = ids;
    if (!target || !valid || busy) return;
    busy = true;
    try {
      let key = existing;
      let label = app.categories.find((c) => c.key === existing)?.name ?? existing;
      if (mode === 'new') {
        key = await api.addCategory(name.trim(), parent || null, definition.trim());
        label = name.trim();
        await app.refresh();
      }
      const t = await api.teach(key, target);
      app.teaching = null;
      toast.show(`Taught ${label} with ${n(t.examples)} ${t.examples === 1 ? 'example' : 'examples'}. ${t.proposed ? `${n(t.proposed)} proposals wait in Review.` : 'No other items look like them yet.'}`, {
        action: t.proposed ? { label: 'Open Review', run: () => app.go('review') } : undefined,
      });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet open={ids != null} title="Teach a category" width={460} onclose={() => (app.teaching = null)}>
  {#if !ready}
    <p class="lead">Teaching needs the image model, which runs on this computer. Download it in Settings › Models and data.</p>
  {:else}
    <p class="lead">
      The {ids?.length === 1 ? 'selected item becomes an example' : `${n(ids?.length ?? 0)} selected items become examples`}: they move to the category, Roll learns
      what they look like and proposes similar items in Review. Nothing is sorted without you.
    </p>
    {#if (ids?.length ?? 0) < 10}
      <p class="hint" data-testid="teach-few">10 to 30 examples work best. Find similar items to add more.</p>
    {/if}
    <Segmented
      label="Category"
      options={[
        { value: 'new', label: 'New category' },
        { value: 'existing', label: 'Existing category' },
      ]}
      bind:value={mode}
    />
    {#if mode === 'existing'}
      <label class="field">
        <span>Category</span>
        <select bind:value={existing} aria-label="Category" data-testid="teach-existing">
          <option value="" disabled>Choose…</option>
          {#each top as c (c.id)}
            <option value={c.key}>{c.name}</option>
            {#each subs(c.id) as s (s.id)}<option value={s.key}>{c.name} › {s.name}</option>{/each}
          {/each}
        </select>
      </label>
    {:else}
      <label class="field">
        <span>Name</span>
        <Input bind:value={name} placeholder="Pokémon GO" aria-label="Name" data-testid="teach-name" />
      </label>
      <label class="field">
        <span>Inside</span>
        <select bind:value={parent} aria-label="Inside">
          <option value="">Nothing (a category of its own)</option>
          {#each top as c (c.id)}<option value={c.key}>{c.name}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>What belongs</span>
        <Input bind:value={definition} placeholder="Screenshots of the game Pokémon GO" aria-label="What belongs" />
      </label>
    {/if}
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (app.teaching = null)}>Cancel</Button>
    <Button variant="primary" disabled={!ready || !valid || busy} onclick={() => void submit()} data-testid="teach-submit">
      {busy ? 'Teaching…' : `Teach with ${n(ids?.length ?? 0)} ${ids?.length === 1 ? 'example' : 'examples'}`}
    </Button>
  {/snippet}
</Sheet>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: var(--t13);
  }
  .hint {
    margin: 0 0 12px;
    font-size: var(--t13);
  }
  .field {
    display: grid;
    gap: 4px;
    margin-top: 12px;
    font-size: var(--t13);
  }
  .field span {
    color: var(--muted);
  }
  select {
    height: 32px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    font-size: var(--t13);
  }
</style>
