<script lang="ts">
  import { api } from '../ipc/api';
  import Grid from '../grid/Grid.svelte';
  import { app } from '../state/app.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';

  const indexing = $derived(app.activeJobs.some((j) => j.kind === 'scan'));
  const filtered = $derived(Object.values(app.filters).some((v) => (Array.isArray(v) ? v.length : v != null && v !== false)));
</script>

<Grid
  query={app.query}
  onopen={(ids, index, rect) => viewer.open(ids, index, rect)}
  onquicklook={(ids, index) => viewer.open(ids, index, null, true)}
>
  {#snippet empty()}
    {#if filtered}
      <EmptyState text="Nothing matches these filters.">
        {#snippet action()}<Button onclick={() => (app.filters = {})}>Clear filters</Button>{/snippet}
      </EmptyState>
    {:else if indexing}
      <EmptyState text="Reading your library. Photos appear here as they are found." />
    {:else if app.roots.length === 0}
      <EmptyState text="Add a folder with photos to start.">
        {#snippet action()}
          <Button
            variant="primary"
            onclick={async () => {
              const p = await api.pickFolder('Choose the library folder');
              if (p) await api.addRoot(p);
            }}>Add folder…</Button
          >
        {/snippet}
      </EmptyState>
    {:else}
      <EmptyState text="Nothing here yet." />
    {/if}
  {/snippet}
</Grid>
