<script lang="ts">
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { n } from '../util/format';

  let busy = $state(false);
  const target = $derived(app.recycling);
  const count = $derived(target === 'all' ? app.counts.trash : (target?.length ?? 0));

  async function confirm() {
    if (!target) return;
    busy = true;
    try {
      const done = await api.emptyTrash(target === 'all' ? null : target);
      selection.clear();
      toast.show(`Sent ${n(done)} ${done === 1 ? 'file' : 'files'} to the Recycle Bin`);
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
      app.recycling = null;
    }
  }
</script>

<Sheet open={target != null} title="Send to the Recycle Bin" width={440} onclose={() => (app.recycling = null)}>
  <p>
    Sends {n(count)}
    {count === 1 ? 'item' : 'items'} and {count === 1 ? 'its' : 'their'} sidecars to the Recycle Bin and removes them from Roll. Roll can't undo
    this, but you can restore the files from the Recycle Bin.
  </p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (app.recycling = null)}>Cancel</Button>
    <Button variant="danger" disabled={busy || count === 0} onclick={confirm} data-testid="confirm-recycle">Send to the Recycle Bin</Button>
  {/snippet}
</Sheet>

<style>
  p {
    margin: 0;
    line-height: 1.5;
  }
</style>
