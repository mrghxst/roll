<script lang="ts">
  import { Merge, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { DupeGroup } from '../ipc/bindings/DupeGroup';
  import { app } from '../state/app.svelte';
  import { undoLast } from '../state/edits';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import { n } from '../util/format';

  let groups = $state<DupeGroup[]>([]);
  let loaded = $state(false);
  /** The chosen keeper per group; the suggestion by default. */
  let keep = $state<Record<string, number>>({});
  let busy = $state(false);

  $effect(() => {
    void app.revision;
    void api.dupeGroups().then((g) => {
      groups = g;
      loaded = true;
    });
  });

  const KIND: Record<string, string> = {
    exact: 'Exact copies',
    copy: 'A smaller copy of a camera photo',
    similar: 'Look alike',
    burst: 'Burst',
  };

  const keeper = (g: DupeGroup) => keep[g.key] ?? g.ids[0]!;

  async function merge(list: DupeGroup[]) {
    if (!list.length) return;
    busy = true;
    try {
      let total = 0;
      for (const g of list) {
        const k = keeper(g);
        total += await api.mergeDupes(
          k,
          g.ids.filter((i) => i !== k),
        );
      }
      groups = groups.filter((g) => !list.includes(g));
      toast.show(`Moved ${n(total)} ${total === 1 ? 'duplicate' : 'duplicates'} to the trash; their dates, places and tags stay with the kept items`, {
        action: list.length === 1 ? { label: 'Undo', run: () => void undoLast() } : undefined,
      });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  async function notDupes(g: DupeGroup) {
    await api.dismiss('dupe', g.key).catch((e) => toast.error(e));
    groups = groups.filter((x) => x !== g);
  }

  const exact = $derived(groups.filter((g) => g.kind === 'exact'));
</script>

<div class="dupes">
  <header>
    <h1>Duplicates</h1>
    <p class="lead">
      Groups of the same picture. Roll suggests which one to keep: the original camera file, then the largest, then the one with the richest
      metadata. Merging moves the others to Roll's trash and gives the kept one their dates, places and tags.
    </p>
    {#if exact.length > 1}
      <Button variant="primary" disabled={busy} onclick={() => void merge(exact)} data-testid="merge-exact">
        <Merge size={16} strokeWidth={1.5} /> Merge all {n(exact.length)} groups of exact copies
      </Button>
    {/if}
  </header>

  {#if loaded && !groups.length}
    <EmptyState text="No duplicates found. Exact copies are checked in the background after each scan." />
  {:else}
    <ul class="groups" data-testid="dupe-groups">
      {#each groups as g (g.key)}
        <li>
          <div class="head">
            <strong>{KIND[g.kind] ?? g.kind}</strong>
            <span class="muted">{n(g.ids.length)} items · keep suggestion: {g.why.replace(/\.$/, '').toLowerCase()}</span>
          </div>
          <div class="items">
            {#each g.ids as id, i (id)}
              <button
                class="item"
                class:kept={keeper(g) === id}
                onclick={() => (keep = { ...keep, [g.key]: id })}
                ondblclick={() => viewer.open(Uint32Array.from(g.ids), i, null)}
                aria-pressed={keeper(g) === id}
                aria-label={keeper(g) === id ? 'Kept' : 'Choose to keep this one'}
              >
                <img src={api.thumbUrl(id, 1)} alt="" loading="lazy" />
                {#if keeper(g) === id}<span class="badge">Keep</span>{/if}
              </button>
            {/each}
          </div>
          <div class="actions">
            {#if g.kind !== 'burst'}
              <Button variant="primary" disabled={busy} onclick={() => void merge([g])} data-testid="merge"><Merge size={14} strokeWidth={1.5} /> Keep one, trash {n(g.ids.length - 1)}</Button>
            {:else}
              <Button disabled={busy} onclick={() => void merge([g])}><Merge size={14} strokeWidth={1.5} /> Keep the chosen frame, trash {n(g.ids.length - 1)}</Button>
            {/if}
            <Button variant="ghost" onclick={() => void notDupes(g)}><X size={14} strokeWidth={1.5} /> Not duplicates</Button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .dupes {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  header,
  .groups {
    max-width: 960px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 16px;
    color: var(--muted);
    max-width: 72ch;
  }
  .groups {
    margin: 20px 0 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 12px;
  }
  li {
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
  .items {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .item {
    position: relative;
    width: 120px;
    height: 120px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: var(--r-btn);
    background: var(--surface);
    cursor: pointer;
  }
  .item.kept {
    border-color: var(--fg);
  }
  .item img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 4px;
  }
  .badge {
    position: absolute;
    left: 6px;
    bottom: 6px;
    padding: 1px 6px;
    border-radius: var(--r-1);
    background: var(--fg);
    color: var(--bg);
    font-size: 11px;
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
</style>
