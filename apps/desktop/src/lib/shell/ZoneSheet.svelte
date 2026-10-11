<script lang="ts">
  import { api } from '../ipc/api';
  import type { ZoneProposal } from '../ipc/bindings/ZoneProposal';
  import { app } from '../state/app.svelte';
  import { offsetLabel } from '../state/places';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { n } from '../util/format';

  let busy = $state(false);
  const zones = $derived(app.zones ?? []);

  async function apply(z: ZoneProposal) {
    busy = true;
    try {
      const done = await api.applyZone(z.ids, z.offset_min, z.zone);
      toast.show(`Set the time zone of ${n(done)} ${done === 1 ? 'item' : 'items'} to ${z.zone}`);
      app.zones = zones.filter((x) => x !== z);
      if (!app.zones.length) app.zones = null;
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet open={zones.length > 0} title="Set time zones too?" width={480} onclose={() => (app.zones = null)}>
  <p class="lead">
    These items now have a place, but their files state no time zone. Setting it keeps the clock time they show and fixes their order against
    photos from other devices.
  </p>
  <ul>
    {#each zones as z (z.zone + z.offset_min)}
      <li>
        <span>{n(z.count)} {z.count === 1 ? 'item' : 'items'} in {z.zone.replaceAll('_', ' ')} <span class="muted mono">{offsetLabel(z.offset_min)}</span></span>
        <Button disabled={busy} onclick={() => void apply(z)} data-testid="apply-zone">Set time zone</Button>
      </li>
    {/each}
  </ul>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (app.zones = null)}>Not now</Button>
  {/snippet}
</Sheet>

<style>
  .lead {
    margin: 0 0 12px;
    color: var(--muted);
    font-size: var(--t13);
    line-height: 1.5;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: var(--t13);
  }
  .muted {
    color: var(--muted);
  }
</style>
