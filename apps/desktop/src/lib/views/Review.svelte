<script lang="ts">
  import { CheckCheck } from '@lucide/svelte';
  import { untrack } from 'svelte';
  import { api } from '../ipc/api';
  import type { ReviewItem } from '../ipc/bindings/ReviewItem';
  import type { ReviewSummary } from '../ipc/bindings/ReviewSummary';
  import { app } from '../state/app.svelte';
  import { undoLast } from '../state/edits';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import Kbd from '../ui/Kbd.svelte';
  import { n } from '../util/format';

  const PAGE = 48;

  let summary = $state<ReviewSummary | null>(null);
  let tab = $state<string | null>(null);
  let items = $state<ReviewItem[]>([]);
  let focus = $state(0);
  let busy = $state(false);
  let grid: HTMLDivElement | undefined = $state();

  async function loadSummary() {
    summary = await api.reviewSummary();
    untrack(() => {
      if (!tab || !summary!.tabs.some((t) => t.key === tab)) tab = summary!.tabs[0]?.key ?? null;
    });
  }

  $effect(() => {
    void app.revision;
    void loadSummary();
  });

  $effect(() => {
    const t = tab;
    void app.revision;
    if (!t) {
      items = [];
      return;
    }
    void api.reviewItems(t, PAGE).then((r) => {
      items = r;
      focus = Math.min(untrack(() => focus), Math.max(0, r.length - 1));
    });
  });

  // Number keys: the top categories, in sidebar order; X is Junk.
  const keyed = $derived(app.categories.filter((c) => c.parent_id === null && c.key !== 'unsorted' && c.key !== 'junk').slice(0, 9));

  async function decide(ids: number[], key: string | null, label: string) {
    if (!ids.length || busy) return;
    busy = true;
    try {
      const done = key === '__accept' ? await api.edit(ids, { kind: 'accept' }) : await api.edit(ids, { kind: 'category', key });
      items = items.filter((i) => !ids.includes(i.id));
      focus = Math.min(focus, Math.max(0, items.length - 1));
      toast.show(`${label} ${done === 1 ? '1 item' : `${n(done)} items`}`, { action: { label: 'Undo (Z)', run: () => void undoLast() } });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (app.view !== 'review' || viewer.ids || app.paletteOpen || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || document.querySelector('dialog[open]')) return;
    const cur = items[focus];
    const cols = Math.max(1, Math.round((grid?.clientWidth ?? 800) / 176));
    if (/^[1-9]$/.test(e.key)) {
      const c = keyed[Number(e.key) - 1];
      if (c && cur) void decide([cur.id], c.key, `Moved to ${c.name}:`);
    } else if (e.key === 'x' || e.key === 'X') {
      if (cur) void decide([cur.id], 'junk', 'Moved to Junk:');
    } else if (e.key === 'Enter') {
      e.preventDefault();
      void decide(
        items.map((i) => i.id),
        '__accept',
        'Accepted',
      );
    } else if (e.key === 'z' || e.key === 'Z') {
      void undoLast();
    } else if (e.key === ' ') {
      e.preventDefault();
      if (items.length) viewer.open(Uint32Array.from(items.map((i) => i.id)), focus, null, true);
    } else if (e.key === 'ArrowRight') focus = Math.min(items.length - 1, focus + 1);
    else if (e.key === 'ArrowLeft') focus = Math.max(0, focus - 1);
    else if (e.key === 'ArrowDown') focus = Math.min(items.length - 1, focus + cols);
    else if (e.key === 'ArrowUp') focus = Math.max(0, focus - cols);
    else return;
    e.preventDefault();
  }

  async function acceptHigh() {
    if (!tab || tab === 'audit') return;
    try {
      const done = await api.acceptAbove(tab, 0.95);
      toast.show(done ? `Accepted ${n(done)} items at 95% or higher` : 'No proposals at 95% or higher here', done ? { action: { label: 'Undo', run: () => void undoLast() } } : {});
    } catch (e) {
      toast.error(e);
    }
  }

  const current = $derived(summary?.tabs.find((t) => t.key === tab));
</script>

<svelte:window {onkeydown} />

<div class="review">
  <header>
    <h1>Review</h1>
    {#if summary}
      <p class="progress mono" data-testid="review-progress">
        Reviewed {n(summary.reviewed)} · Auto-accepted {n(summary.auto_accepted)} · Left {n(summary.left)}
        {#if summary.precision != null && summary.margin != null}
          · estimated {(summary.precision * 100).toFixed(1)}% correct, ±{(summary.margin * 100).toFixed(1)}%
        {:else if summary.audited}
          · {n(summary.audited)} audited so far
        {/if}
      </p>
    {/if}
  </header>

  {#if summary && summary.tabs.length}
    <nav class="tabs" aria-label="Proposed categories">
      {#each summary.tabs as t (t.key)}
        <button class:active={t.key === tab} onclick={() => (tab = t.key)} data-testid="review-tab">
          {t.name} <span class="mono">{n(t.count)}</span>
        </button>
      {/each}
    </nav>
    <div class="bar">
      <p class="legend">
        {#each keyed as c, i (c.key)}<span><Kbd keys={String(i + 1)} /> {c.name}</span>{/each}
        <span><Kbd keys="X" /> Junk</span>
        <span><Kbd keys="Enter" /> accept the page</span>
        <span><Kbd keys="Space" /> preview</span>
        <span><Kbd keys="Z" /> undo</span>
      </p>
      {#if tab !== 'audit'}
        <Button onclick={acceptHigh} data-testid="accept-high"><CheckCheck size={16} strokeWidth={1.5} /> Accept all at 95% or higher</Button>
      {/if}
    </div>
    {#if tab === 'audit'}
      <p class="note">A random sample of items Roll sorted on its own. Enter confirms them; a number key moves one that is wrong. The answers estimate how often Roll is right.</p>
    {/if}
    <div class="grid" bind:this={grid} role="listbox" aria-label="{current?.name ?? ''} proposals" data-testid="review-grid">
      {#each items as it, i (it.id)}
        <button
          class="card"
          class:focus={i === focus}
          role="option"
          aria-selected={i === focus}
          onclick={() => (focus = i)}
          ondblclick={() => viewer.open(Uint32Array.from(items.map((x) => x.id)), i, null)}
        >
          <img src={api.thumbUrl(it.id, it.thumb_v)} alt="" loading="lazy" />
          <span class="conf mono">{Math.round(it.conf * 100)}%</span>
          <span class="why">{it.why || 'No reason recorded.'}</span>
        </button>
      {/each}
    </div>
  {:else if summary}
    <EmptyState text="Nothing to review. Roll proposes categories after it reads the library; certain rules decide without review." />
  {/if}
</div>

<style>
  .review {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 24px 32px 0;
    overflow: hidden;
  }
  h1 {
    margin: 0 0 6px;
    font-size: var(--t24);
    font-weight: 600;
  }
  .progress {
    margin: 0 0 16px;
    color: var(--muted);
    font-size: var(--t12);
  }
  .tabs {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--muted);
    font-size: var(--t13);
    cursor: pointer;
  }
  .tabs button.active {
    color: var(--fg);
    border-bottom-color: var(--fg);
  }
  .tabs .mono {
    margin-left: 4px;
    font-size: var(--t12);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 0;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
    flex: 1;
    margin: 0;
    color: var(--muted);
    font-size: var(--t12);
  }
  .note {
    margin: 0 0 10px;
    color: var(--muted);
    font-size: var(--t13);
  }
  .grid {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 12px;
    align-content: start;
    padding-bottom: 32px;
  }
  .card {
    position: relative;
    display: grid;
    gap: 6px;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--bg);
    color: var(--fg);
    text-align: left;
    cursor: pointer;
  }
  .card.focus {
    border-color: var(--fg);
    box-shadow: 0 0 0 1px var(--fg);
  }
  .card img {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: var(--r-1);
    background: var(--surface);
  }
  .conf {
    position: absolute;
    top: 10px;
    right: 10px;
    padding: 1px 6px;
    border-radius: var(--r-1);
    background: var(--bg);
    font-size: 11px;
  }
  .why {
    color: var(--muted);
    font-size: var(--t12);
    line-height: 1.35;
  }
</style>
