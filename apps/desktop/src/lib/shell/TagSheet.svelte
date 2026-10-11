<script lang="ts">
  import { app } from '../state/app.svelte';
  import { addTag } from '../state/edits';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Sheet from '../ui/Sheet.svelte';

  let text = $state('');
  let active = $state(-1);
  let el: HTMLInputElement | undefined = $state();

  const ids = $derived(app.tagging);
  const suggestions = $derived.by(() => {
    const t = text.trim().toLowerCase();
    const all = app.tags.filter((x) => !x.path.startsWith('Category/'));
    return (t ? all.filter((x) => x.path.toLowerCase().includes(t)) : all).slice(0, 8);
  });

  $effect(() => {
    if (ids) {
      text = '';
      active = -1;
      queueMicrotask(() => el?.focus());
    }
  });

  async function submit(path: string) {
    const p = path.trim().replace(/\s*\/\s*/g, '/');
    const target = ids;
    if (!p || !target) return;
    app.tagging = null;
    await addTag(target, p);
  }
</script>

<Sheet open={ids != null} title="Add a tag" width={420} onclose={() => (app.tagging = null)}>
  <p class="lead">Tags {ids?.length === 1 ? 'this item' : `${ids?.length ?? 0} items`}. Use a slash for levels, for example Trips/Lake Garda.</p>
  <Input
    bind:el
    bind:value={text}
    placeholder="Tag"
    aria-label="Tag"
    data-testid="tag-input"
    onkeydown={(e) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        active = Math.min(suggestions.length - 1, active + 1);
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        active = Math.max(-1, active - 1);
      } else if (e.key === 'Enter') {
        e.preventDefault();
        void submit(active >= 0 ? suggestions[active]!.path : text);
      }
    }}
  />
  {#if suggestions.length}
    <ul class="suggest" role="listbox" aria-label="Existing tags">
      {#each suggestions as s, i (s.id)}
        <li role="option" aria-selected={i === active}>
          <button class:active={i === active} onclick={() => void submit(s.path)}>
            <span>{s.path}</span><span class="mono muted">{s.count}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (app.tagging = null)}>Cancel</Button>
    <Button variant="primary" disabled={!text.trim()} onclick={() => void submit(text)}>Add tag</Button>
  {/snippet}
</Sheet>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: var(--t13);
  }
  .suggest {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .suggest button {
    display: flex;
    justify-content: space-between;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--fg);
    font-size: var(--t13);
    cursor: pointer;
  }
  .suggest button:hover,
  .suggest button.active {
    background: var(--surface);
  }
  .muted {
    color: var(--muted);
  }
</style>
