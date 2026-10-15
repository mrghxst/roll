<script lang="ts">
  import { Cpu, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import { bytes } from '../util/format';

  let { onready }: { onready?: () => void } = $props();

  const pack = $derived(app.pack('local-ai'));
  const busy = $derived(pack?.state === 'downloading' || pack?.state === 'installing');
  // Not state: the effect below both reads and writes it.
  let was: string | undefined;

  // Tell the page once the download is done.
  $effect(() => {
    const now = pack?.state;
    if (was && was !== 'installed' && now === 'installed') onready?.();
    was = now;
  });

  const downloadsOn = $derived(!!app.settings?.network.downloads);

  async function setup() {
    try {
      // The button said so: downloads go on with this click.
      if (!downloadsOn && app.settings) await app.saveSettings({ network: { ...app.settings.network, downloads: true } });
      await api.setupLocalAi();
      await app.loadPacks();
    } catch (e) {
      toast.error(e);
    }
  }
</script>

{#if pack && pack.state !== 'installed'}
  <div class="setup" data-testid="local-ai-setup">
    <Cpu size={18} strokeWidth={1.5} />
    <div class="text">
      <strong>Local AI in one click</strong>
      <p>
        Qwen3-VL 4B on llama.cpp's server, about {bytes(pack.size)}. Roll starts it when needed and stops it when idle; nothing leaves this computer and
        nothing else needs installing. Works best with 6 GB of graphics memory.
      </p>
      <p class="muted">From {pack.hosts.join(' and ')} · {pack.license}{pack.verified ? ' · checked against known checksums' : ''}</p>
      {#if busy}
        <div class="bar" role="progressbar" aria-label="Downloading the local AI" aria-valuemin={0} aria-valuemax={pack.size} aria-valuenow={pack.done}>
          <span style:width="{(pack.done / Math.max(1, pack.size)) * 100}%"></span>
        </div>
        <p class="muted">{pack.state === 'installing' ? 'Unpacking…' : `${bytes(pack.done)} of ${bytes(pack.size)}`}</p>
      {:else if pack.state === 'failed'}
        <p class="warn">{pack.error}</p>
      {/if}
    </div>
    {#if busy}
      <Button variant="ghost" onclick={() => void api.cancelPack('local-ai')}><X size={14} strokeWidth={1.5} /> Stop</Button>
    {:else}
      <Button variant="primary" onclick={() => void setup()} data-testid="setup-local-ai">
        {downloadsOn ? `Download and set up (${bytes(pack.size)})` : `Turn on downloads and set up (${bytes(pack.size)})`}
      </Button>
    {/if}
  </div>
{/if}

<style>
  .setup {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin: 12px 0;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
  }
  .text {
    flex: 1;
    display: grid;
    gap: 4px;
  }
  p {
    margin: 0;
    color: var(--muted);
    font-size: var(--t13);
  }
  .warn {
    color: var(--danger);
  }
  .bar {
    height: 4px;
    margin-top: 4px;
    border-radius: 2px;
    background: var(--surface);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--fg);
  }
</style>
