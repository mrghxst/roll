<script lang="ts">
  import Grid from '../grid/Grid.svelte';
  import { app, mergeQuery } from '../state/app.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import { n } from '../util/format';

  const query = $derived(mergeQuery(app.filters, { trashed: true }));
</script>

<div class="trash">
  <div class="bar">
    <p>
      Items in the trash stay on disk and out of the timeline. Emptying the trash sends the files to the Recycle Bin, where they can still be
      restored.
    </p>
    <Button variant="secondary" disabled={app.counts.trash === 0} onclick={() => (app.recycling = 'all')} data-testid="empty-trash">
      Empty trash ({n(app.counts.trash)})
    </Button>
  </div>
  <div class="grid">
    <Grid {query} headers={false} onopen={(ids, i, rect) => viewer.open(ids, i, rect)} onquicklook={(ids, i) => viewer.open(ids, i, null, true)}>
      {#snippet empty()}
        <EmptyState text="The trash is empty." />
      {/snippet}
    </Grid>
  </div>
</div>

<style>
  .trash {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 52px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
  }
  .bar p {
    flex: 1;
    margin: 0;
    max-width: 72ch;
    color: var(--muted);
    font-size: var(--t13);
  }
  .grid {
    flex: 1;
    min-height: 0;
    position: relative;
  }
</style>
