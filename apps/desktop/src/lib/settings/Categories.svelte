<script lang="ts">
  import { ChevronDown, ChevronRight, Plus, Trash2 } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { Category } from '../ipc/bindings/Category';
  import type { CategoryPatch } from '../ipc/bindings/CategoryPatch';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Input from '../ui/Input.svelte';
  import Switch from '../ui/Switch.svelte';
  import { n } from '../util/format';

  let open = $state<string | null>(null);
  let adding = $state<{ parent: string | null; name: string; definition: string } | null>(null);

  const top = $derived(app.categories.filter((c) => c.parent_id === null));
  const subs = (c: Category) => app.categories.filter((x) => x.parent_id === c.id);

  async function patch(c: Category, p: Partial<CategoryPatch>) {
    try {
      await api.updateCategory(c.key, p);
      await app.refresh();
    } catch (e) {
      toast.error(e);
    }
  }

  async function add() {
    if (!adding) return;
    try {
      const key = await api.addCategory(adding.name, adding.parent, adding.definition);
      adding = null;
      await app.refresh();
      open = key;
    } catch (e) {
      toast.error(e);
    }
  }

  async function remove(c: Category) {
    if (!confirm(`Deletes ${c.name}. Its ${n(c.count)} items become unsorted. Files stay where they are.`)) return;
    try {
      await api.deleteCategory(c.key);
      await app.refresh();
    } catch (e) {
      toast.error(e);
    }
  }

  const value = (e: Event) => (e.currentTarget as HTMLInputElement | HTMLTextAreaElement).value;
</script>

{#snippet row(c: Category, depth: number)}
  {@const expanded = open === c.key}
  <li class:sub={depth > 0}>
    <button class="head" onclick={() => (open = expanded ? null : c.key)} aria-expanded={expanded}>
      {#if expanded}<ChevronDown size={14} strokeWidth={1.5} />{:else}<ChevronRight size={14} strokeWidth={1.5} />{/if}
      <span class="name">{c.name}</span>
      <span class="mono muted folder">{c.key === 'unsorted' ? 'stays where it is' : depth > 0 && !c.own_folder ? 'tag only' : (c.folder_template ?? 'stays where it is')}</span>
      <span class="mono muted count">{n(c.count)}</span>
    </button>
    {#if expanded}
      <div class="edit">
        <label class="field">
          <span>Name</span>
          <Input value={c.name} onchange={(e) => void patch(c, { name: value(e) })} aria-label="Name" />
        </label>
        <label class="field">
          <span>What belongs here</span>
          <textarea rows="2" value={c.definition} onchange={(e) => void patch(c, { definition: value(e) })} aria-label="What belongs here"></textarea>
          <span class="hint">Also used to explain the category to AI models.</span>
        </label>
        {#if depth > 0}
          <label class="toggle">
            <Switch label="Own folder" checked={c.own_folder} onchange={(v) => void patch(c, { own_folder: v })} />
            <span>Own folder <span class="muted">Off: items stay in the parent's folder and get a tag.</span></span>
          </label>
        {/if}
        {#if c.key !== 'unsorted' && (depth === 0 || c.own_folder)}
          <label class="field">
            <span>Folder</span>
            <Input
              mono
              value={c.folder_template ?? ''}
              placeholder={depth > 0 ? '{year}/Parent/{subcategory}' : 'Leave empty to keep items where they are'}
              onchange={(e) => void patch(c, { folder_template: value(e) })}
              aria-label="Folder template"
            />
            <span class="hint">Tokens: {'{year}'} {'{month}'} {'{month_name}'} {'{date}'} {'{category}'} {'{subcategory}'} {'{source}'} {'{camera}'}</span>
          </label>
        {/if}
        <div class="row">
          {#if depth === 0 && c.key !== 'unsorted'}
            <Button variant="ghost" onclick={() => (adding = { parent: c.key, name: '', definition: '' })}><Plus size={14} strokeWidth={1.5} /> Add subcategory</Button>
          {/if}
          <span class="spacer"></span>
          {#if !c.builtin}
            <IconButton label="Delete {c.name}" onclick={() => void remove(c)}><Trash2 size={16} strokeWidth={1.5} /></IconButton>
          {/if}
        </div>
      </div>
    {/if}
  </li>
  {#each subs(c) as s (s.id)}{@render row(s, depth + 1)}{/each}
{/snippet}

<ul class="cats" data-testid="categories">
  {#each top as c (c.id)}{@render row(c, 0)}{/each}
</ul>

{#if adding}
  <div class="add" role="group" aria-label="New category">
    <strong>{adding.parent ? `New subcategory of ${app.categories.find((c) => c.key === adding?.parent)?.name}` : 'New category'}</strong>
    <Input bind:value={adding.name} placeholder="Name" aria-label="Name" />
    <textarea rows="2" bind:value={adding.definition} placeholder="What belongs here, in one line" aria-label="What belongs here"></textarea>
    <div class="row">
      <Button variant="ghost" onclick={() => (adding = null)}>Cancel</Button>
      <Button variant="primary" disabled={!adding.name.trim()} onclick={add}>Add</Button>
    </div>
  </div>
{:else}
  <Button onclick={() => (adding = { parent: null, name: '', definition: '' })}><Plus size={16} strokeWidth={1.5} /> Add category</Button>
{/if}

<style>
  .cats {
    margin: 0 0 16px;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  li {
    border-bottom: 1px solid var(--border);
  }
  li:last-child {
    border-bottom: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 40px;
    padding: 0 12px;
    border: 0;
    background: transparent;
    color: var(--fg);
    font-size: var(--t13);
    text-align: left;
    cursor: pointer;
  }
  .sub .head {
    padding-left: 32px;
  }
  .head:hover {
    background: var(--surface);
  }
  .name {
    min-width: 120px;
  }
  .folder {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted,
  .hint {
    color: var(--muted);
  }
  .count,
  .hint {
    font-size: var(--t12);
  }
  .edit {
    display: grid;
    gap: 12px;
    padding: 4px 16px 16px 34px;
  }
  .sub .edit {
    padding-left: 54px;
  }
  .field {
    display: grid;
    gap: 4px;
    font-size: var(--t13);
  }
  .toggle {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    font-size: var(--t13);
  }
  .toggle > span {
    display: grid;
    gap: 2px;
  }
  textarea {
    width: 100%;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    font-size: var(--t13);
    resize: vertical;
    outline: none;
    user-select: text;
  }
  textarea:focus {
    border-color: var(--fg);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .spacer {
    flex: 1;
  }
  .add {
    display: grid;
    gap: 8px;
    max-width: 480px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .add .row {
    justify-content: flex-end;
  }
</style>
