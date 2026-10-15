<script lang="ts">
  import { Merge, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { DupeGroup } from '../ipc/bindings/DupeGroup';
  import type { ExactPlan } from '../ipc/bindings/ExactPlan';
  import type { ShotScore } from '../ipc/bindings/ShotScore';
  import Sheet from '../ui/Sheet.svelte';
  import { app } from '../state/app.svelte';
  import { undoLast } from '../state/edits';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import { bytes, n } from '../util/format';

  let groups = $state<DupeGroup[]>([]);
  let loaded = $state(false);
  /** The chosen keepers per group (the first takes the others' dates,
   * places and tags); the suggestion by default. */
  let keep = $state<Record<string, number[]>>({});
  /** Series ranked by picture quality (bursts and look-alikes). */
  let ranks = $state<Record<string, ShotScore[]>>({});
  let busy = $state(false);

  $effect(() => {
    void app.revision;
    void api.dupeGroups().then(async (g) => {
      groups = g;
      loaded = true;
      // Rank the series, a few at a time, so the first ones show quickly.
      for (const s of g.filter((x) => x.kind === 'burst' || x.kind === 'similar').slice(0, 40)) {
        // A group's key names its members: ranked once.
        if (ranks[s.key]) continue;
        const r = await api.bestShots(s.ids).catch(() => null);
        if (r?.length) ranks = { ...ranks, [s.key]: r };
      }
    });
  });

  /** What the ranking looks at, with the packs that are installed. */
  const rankedBy = $derived.by(() => {
    const parts = ['sharpness', 'exposure'];
    if (app.pack('faces')?.state === 'installed') parts.push('faces');
    if (app.pack('image-model')?.state === 'installed') parts.push('look');
    return `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`;
  });
  const series = (g: DupeGroup) => g.kind === 'burst' || g.kind === 'similar';
  /** Display order: best first once ranked. */
  const order = (g: DupeGroup) => ranks[g.key]?.map((r) => r.id).filter((id) => g.ids.includes(id)) ?? g.ids;
  const scoreOf = (g: DupeGroup, id: number) => ranks[g.key]?.find((r) => r.id === id);

  const KIND: Record<string, string> = {
    exact: 'Exact copies',
    copy: 'A smaller copy of a camera photo',
    similar: 'Look alike',
    burst: 'Burst',
  };

  const keepers = (g: DupeGroup): number[] =>
    keep[g.key] ?? (ranks[g.key] ? ranks[g.key]!.filter((r) => r.keep).map((r) => r.id) : [g.ids[0]!]);
  const keeper = (g: DupeGroup) => keepers(g)[0] ?? g.ids[0]!;

  /** A series can keep several frames; other groups keep one. */
  function choose(g: DupeGroup, id: number) {
    if (!series(g)) {
      keep = { ...keep, [g.key]: [id] };
      return;
    }
    const cur = keepers(g);
    const next = cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id];
    keep = { ...keep, [g.key]: next.length ? next : cur };
  }

  async function merge(list: DupeGroup[]) {
    if (!list.length) return;
    busy = true;
    try {
      let total = 0;
      for (const g of list) {
        const ks = keepers(g);
        total += await api.mergeDupes(
          ks[0]!,
          g.ids.filter((i) => !ks.includes(i)),
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

  // Merge all exact copies: preview first, then one step with one undo.
  let plan = $state<ExactPlan | null>(null);

  async function previewAll() {
    try {
      plan = await api.exactPlan();
    } catch (e) {
      toast.error(e);
    }
  }

  async function mergeAll() {
    busy = true;
    try {
      const done = await api.mergeAllExact();
      plan = null;
      groups = groups.filter((g) => g.kind !== 'exact');
      toast.show(`Moved ${n(done)} exact ${done === 1 ? 'copy' : 'copies'} to the trash; one of each stays, with their dates, places and tags`, {
        action: { label: 'Undo', run: () => void undoLast() },
      });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="dupes">
  <header>
    <h1>Duplicates</h1>
    <p class="lead">
      Groups of the same picture. Roll suggests which one to keep: the original camera file, then the largest, then the one with the richest
      metadata. Merging moves the others to Roll's trash and gives the kept one their dates, places and tags.
    </p>
    {#if exact.length > 1}
      <Button variant="primary" disabled={busy} onclick={() => void previewAll()} data-testid="merge-exact">
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
          {#if series(g) && ranks[g.key]}
            <p class="muted ranked" data-testid="ranked">Best first, by {rankedBy}. Click to choose which to keep.</p>
          {/if}
          <div class="items">
            {#each order(g) as id, i (id)}
              {@const s = scoreOf(g, id)}
              <div class="cell">
                <button
                  class="item"
                  class:kept={keepers(g).includes(id)}
                  onclick={() => choose(g, id)}
                  ondblclick={() => viewer.open(Uint32Array.from(order(g)), i, null)}
                  aria-pressed={keepers(g).includes(id)}
                  aria-label={keepers(g).includes(id) ? 'Kept' : 'Choose to keep this one'}
                >
                  <img src={api.thumbUrl(id, 1)} alt="" loading="lazy" />
                  {#if keepers(g).includes(id)}<span class="badge">Keep</span>{/if}
                </button>
                {#if s?.notes.length}<span class="notes">{s.notes.join(' · ')}</span>{/if}
              </div>
            {/each}
          </div>
          <div class="actions">
            {#if !series(g)}
              <Button variant="primary" disabled={busy} onclick={() => void merge([g])} data-testid="merge"><Merge size={14} strokeWidth={1.5} /> Keep one, trash {n(g.ids.length - 1)}</Button>
            {:else}
              <Button disabled={busy} onclick={() => void merge([g])} data-testid="keep-best">
                <Merge size={14} strokeWidth={1.5} /> Keep {n(keepers(g).length)}, trash {n(g.ids.length - keepers(g).length)}
              </Button>
            {/if}
            <Button variant="ghost" onclick={() => void notDupes(g)}><X size={14} strokeWidth={1.5} /> Not duplicates</Button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<Sheet open={plan != null} title="Merge all exact copies" width={560} onclose={() => (plan = null)}>
  {#if plan}
    <p class="lead" data-testid="exact-plan">
      {n(plan.copies)} {plan.copies === 1 ? 'copy' : 'copies'} in {n(plan.groups)} {plan.groups === 1 ? 'group' : 'groups'} go to Roll's trash, {bytes(plan.bytes)}. In each
      group the same file stays once: the one in your own folders, not in Downloads, backups or with a copy-like name. Its dates, places and tags
      are kept, and one undo brings everything back. Files are only deleted when you empty the trash.
    </p>
    <ul class="plan">
      {#each plan.samples as s, i (i)}
        <li>
          <span class="kept">{s[0]}</span>
          {#each s.slice(1) as o (o)}<span class="muted">{o}</span>{/each}
        </li>
      {/each}
    </ul>
    {#if plan.groups > plan.samples.length}<p class="muted">And {n(plan.groups - plan.samples.length)} more groups.</p>{/if}
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (plan = null)}>Cancel</Button>
    <Button variant="primary" disabled={busy || !plan?.copies} onclick={() => void mergeAll()} data-testid="merge-exact-confirm">
      <Merge size={16} strokeWidth={1.5} /> Move {n(plan?.copies ?? 0)} copies to the trash
    </Button>
  {/snippet}
</Sheet>

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
  .plan {
    display: grid;
    gap: 8px;
    margin: 12px 0;
    padding: 0;
    list-style: none;
    font: 12px/1.5 var(--mono);
  }
  .plan li {
    display: grid;
  }
  .plan .kept::before {
    content: 'Keep  ';
    color: var(--muted);
  }
  .plan .muted::before {
    content: 'Trash ';
  }
  .cell {
    display: grid;
    gap: 2px;
    align-content: start;
  }
  .notes {
    color: var(--muted);
    font-size: 11px;
  }
  .ranked {
    margin: 0 0 6px;
    font-size: var(--t12);
  }
</style>
