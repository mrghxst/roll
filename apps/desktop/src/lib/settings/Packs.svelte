<script lang="ts">
  import { Download, Square, Trash2 } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { PackStatus } from '../ipc/bindings/PackStatus';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import { bytes } from '../util/format';

  async function run(f: () => Promise<void>) {
    try {
      await f();
    } catch (e) {
      toast.error(e);
    }
  }

  function remove(p: PackStatus) {
    if (!confirm(`Deletes ${p.title.toLowerCase()} (${bytes(p.size)}) from this computer. You can download it again later.`)) return;
    void run(() => api.removePack(p.id));
  }

  const pct = (p: PackStatus) => (p.size ? Math.round((p.done / p.size) * 100) : 0);
</script>

<ul class="packs" data-testid="packs">
  {#each app.packs as p (p.id)}
    <li data-pack={p.id}>
      <div class="head">
        <strong>{p.title}</strong>
        <span class="muted">{bytes(p.size)} · {p.license} · {p.source}</span>
      </div>
      <p class="muted">{p.purpose}</p>
      {#if p.state === 'downloading' || p.state === 'installing'}
        <div class="progress" role="progressbar" aria-label="Downloading {p.title}" aria-valuenow={pct(p)} aria-valuemin={0} aria-valuemax={100}>
          <div style:width="{pct(p)}%"></div>
        </div>
        <div class="row">
          <span class="muted mono">
            {p.state === 'installing' ? 'Installing…' : `${bytes(p.done)} of ${bytes(p.size)}`}
          </span>
          {#if p.state === 'downloading'}
            <Button variant="ghost" onclick={() => void run(() => api.cancelPack(p.id))}><Square size={14} strokeWidth={1.5} /> Stop</Button>
          {/if}
        </div>
      {:else}
        {#if p.error}<p class="error" role="alert">The download failed: {p.error}</p>{/if}
        <div class="row">
          {#if p.state === 'installed'}
            <span class="ok">Installed</span>
            <Button variant="ghost" onclick={() => remove(p)}><Trash2 size={14} strokeWidth={1.5} /> Remove</Button>
          {:else}
            <Button onclick={() => (app.packAsk = p.id)} data-testid="download-{p.id}">
              <Download size={14} strokeWidth={1.5} />
              {p.state === 'outdated' ? 'Update' : p.state === 'failed' ? 'Try again' : p.done > 0 ? 'Continue download' : 'Download'}…
            </Button>
            {#if p.state === 'failed'}
              <Button variant="ghost" onclick={() => void run(() => api.cancelPack(p.id))}>Dismiss</Button>
            {:else if p.state === 'outdated'}
              <span class="muted">A newer version is listed</span>
            {:else if p.done > 0}
              <span class="muted mono">{bytes(p.done)} downloaded</span>
            {/if}
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .packs {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  li {
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
    font-size: var(--t13);
  }
  li:last-child {
    border-bottom: 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 12px;
  }
  .head strong {
    font-weight: 500;
  }
  p {
    margin: 4px 0 10px;
    line-height: 1.45;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .progress {
    height: 4px;
    margin-bottom: 8px;
    border-radius: 2px;
    background: var(--surface);
    overflow: hidden;
  }
  .progress div {
    height: 100%;
    background: var(--fg);
    transition: width 200ms var(--ease);
  }
  .ok {
    color: var(--muted);
  }
  .error {
    color: var(--danger, var(--fg));
  }
</style>
