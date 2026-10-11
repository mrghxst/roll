<script lang="ts">
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { bytes } from '../util/format';

  let busy = $state(false);
  const p = $derived(app.packAsk ? app.pack(app.packAsk) : undefined);
  const off = $derived(!app.settings?.network.downloads);
  const left = $derived(p ? Math.max(0, p.size - p.done) : 0);

  async function start() {
    if (!p) return;
    busy = true;
    try {
      if (off) await app.saveSettings({ network: { ...app.settings!.network, downloads: true } });
      await api.downloadPack(p.id);
      app.packAsk = null;
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet open={p != null} title="Download {p?.title.toLowerCase() ?? ''}" width={480} onclose={() => (app.packAsk = null)}>
  {#if p}
    <p>{p.purpose}</p>
    <dl>
      <dt>Size</dt>
      <dd>{bytes(p.size)}{p.done > 0 && p.done < p.size ? `, ${bytes(left)} still to download` : ''}</dd>
      <dt>From</dt>
      <dd class="mono">{p.hosts.join(', ')}</dd>
      <dt>License</dt>
      <dd>{p.license}</dd>
    </dl>
    <p class="muted">
      {#if p.verified}
        Roll checks every file against the SHA-256 listed in the app before using it, and deletes files that don't match.
      {:else}
        These files change daily at the source, so they have no fixed checksum. Roll checks their size and format instead.
      {/if}
      Only the file requests reach the server; nothing about your library is sent.
    </p>
    {#if off}
      <p class="off" role="note">Downloads are off in Settings › Network. Downloading turns them on.</p>
    {/if}
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (app.packAsk = null)}>Cancel</Button>
    <Button variant="primary" disabled={busy || !p} onclick={start} data-testid="confirm-download">
      {off ? 'Turn on downloads and download' : 'Download'} {p ? bytes(left || p.size) : ''}
    </Button>
  {/snippet}
</Sheet>

<style>
  p {
    margin: 0 0 12px;
    line-height: 1.5;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 16px;
    margin: 0 0 12px;
    font-size: var(--t13);
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
  .muted {
    font-size: var(--t13);
  }
  .off {
    margin: 0;
    padding: 8px 12px;
    border-radius: var(--r-btn);
    background: var(--surface);
    font-size: var(--t13);
  }
</style>
