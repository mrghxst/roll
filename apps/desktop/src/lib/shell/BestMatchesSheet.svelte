<script lang="ts">
  import { Sparkles } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { BestMatches } from '../ipc/bindings/BestMatches';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { n } from '../util/format';

  let result = $state<BestMatches | null>(null);
  let loading = $state(false);
  let ranking = $state(false);

  // "Better ranking" needs a decision model.
  const canRank = $derived(!!app.settings && app.settings.ai_mode !== 'off' && !!app.settings.ai_decision_model);
  const ranked = $derived(!!result?.scores.some((s) => s != null));

  $effect(() => {
    if (!app.bestOpen) return;
    const q = app.query;
    result = null;
    loading = true;
    api
      .bestMatches(q)
      .then((r) => (result = r))
      .catch((e) => toast.error(e))
      .finally(() => (loading = false));
  });

  async function rank() {
    if (ranking) return;
    ranking = true;
    try {
      result = await api.rerank(app.query);
      if (result.cost_usd > 0) toast.show(`Ranked by the model for $${result.cost_usd.toFixed(4)}.`);
    } catch (e) {
      toast.error(e);
    } finally {
      ranking = false;
    }
  }

  /** About 1.5 s a picture with an 8B model on an RTX 2080 Ti (docs/benchmarks.md). */
  function localTime(pictures: number) {
    const s = pictures * 1.5;
    return s < 90 ? `${Math.max(10, Math.round(s / 10) * 10)} s` : `${Math.round(s / 60)} min`;
  }

  const sends = $derived(
    app.settings?.ai_mode === 'openrouter'
      ? `Sends the words and ${n(result?.ids.length ?? 0)} pictures (320 px, no metadata) to OpenRouter, usually under $0.001.`
      : `Asks the local model; nothing leaves this computer. About ${localTime(result?.ids.length ?? 0)} for ${n(result?.ids.length ?? 0)} pictures on a mid-range graphics card.`,
  );
</script>

<Sheet open={app.bestOpen} title="Best matches" width={680} onclose={() => (app.bestOpen = false)}>
  {#if loading}
    <p class="lead">Looking…</p>
  {:else if !result?.ids.length}
    <p class="lead" data-testid="best-empty">Nothing matches the words of this search. Filters alone have no order to rank by.</p>
  {:else}
    <p class="lead" data-testid="best-lead">
      The {n(result.ids.length)} items that match “{result.words}” best, best first{ranked ? ', as the model rated them' : ', by the words and by what the pictures show'}. Click one to open it.
    </p>
    <div class="grid" data-testid="best-grid">
      {#each result.ids as id, i (id)}
        {@const s = result.scores[i]}
        <button class="item" onclick={() => viewer.open(Uint32Array.from(result!.ids), i, null)} aria-label="Open match {i + 1}">
          <img src={api.thumbUrl(id, 1)} alt="" loading="lazy" />
          {#if s != null}<span class="score" class:low={s < 5}>{s}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
  {#snippet footer()}
    {#if canRank && result?.ids.length}
      <span class="sends">{sends}</span>
      <Button disabled={ranking} onclick={() => void rank()} data-testid="better-ranking">
        <Sparkles size={14} strokeWidth={1.5} />
        {ranking ? 'Ranking…' : ranked ? 'Rank again' : 'Better ranking'}
      </Button>
    {/if}
    <Button variant="primary" onclick={() => (app.bestOpen = false)}>Done</Button>
  {/snippet}
</Sheet>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: var(--t13);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));
    gap: 4px;
    max-height: 60vh;
    overflow: auto;
  }
  .item {
    position: relative;
    aspect-ratio: 1;
    padding: 0;
    border: 0;
    border-radius: var(--r-1);
    overflow: hidden;
    background: var(--surface);
    cursor: pointer;
  }
  .item img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .item:focus-visible {
    outline: 2px solid var(--fg);
    outline-offset: 1px;
  }
  .score {
    position: absolute;
    right: 4px;
    bottom: 4px;
    min-width: 18px;
    padding: 0 4px;
    border-radius: 3px;
    background: var(--bg);
    color: var(--fg);
    font-size: 11px;
    text-align: center;
  }
  .score.low {
    color: var(--muted);
  }
  .sends {
    margin-right: auto;
    color: var(--muted);
    font-size: var(--t12);
  }
</style>
