<script lang="ts">
  import { Pause, Play, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import { eta, n } from '../util/format';

  const spend = $derived(app.jobs.reduce((s, j) => s + j.cost_usd, 0));
  const STATE: Record<string, string> = { queued: 'Waiting', running: 'Running', paused: 'Paused', done: 'Done', failed: 'Failed', cancelled: 'Stopped' };
</script>

<div class="activity">
  <header>
    <h1>Activity</h1>
    <span class="muted mono">AI spend so far ${spend.toFixed(2)}</span>
  </header>
  {#if app.jobs.length === 0}
    <EmptyState text="No background work right now." />
  {:else}
    <ul>
      {#each app.jobs as j (j.id)}
        {@const pct = j.total ? Math.min(100, (j.done / j.total) * 100) : j.state === 'done' ? 100 : 0}
        <li class:failed={j.state === 'failed'}>
          <div class="line">
            <span class="label">{j.label}</span>
            <span class="state muted">{STATE[j.state] ?? j.state}</span>
            {#if j.state === 'running'}
              <IconButton label="Pause" size={28} onclick={() => void api.jobControl(j.id, 'pause')}><Pause size={14} strokeWidth={1.5} /></IconButton>
            {:else if j.state === 'paused'}
              <IconButton label="Resume" size={28} onclick={() => void api.jobControl(j.id, 'resume')}><Play size={14} strokeWidth={1.5} /></IconButton>
            {/if}
            {#if j.state === 'running' || j.state === 'paused' || j.state === 'queued'}
              <IconButton label="Stop" size={28} onclick={() => void api.jobControl(j.id, 'cancel')}><X size={14} strokeWidth={1.5} /></IconButton>
            {/if}
          </div>
          <div class="bar" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={j.label}>
            <span style:width="{pct}%"></span>
          </div>
          <div class="meta mono muted">
            {#if j.total}{n(j.done)} of {n(j.total)}{/if}
            {#if j.state === 'running' && j.rate > 0}· {j.rate >= 10 ? Math.round(j.rate) : j.rate.toFixed(1)} per second{/if}
            {#if j.state === 'running' && j.eta_secs != null}· about {eta(j.eta_secs)} left{/if}
            {#if j.cost_usd > 0}· ${j.cost_usd.toFixed(3)}{/if}
          </div>
          {#if j.error}<p class="error">{j.error}</p>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .activity {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    max-width: 720px;
    margin-bottom: 16px;
  }
  h1 {
    margin: 0;
    font-size: var(--t24);
    font-weight: 600;
  }
  ul {
    max-width: 720px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    padding: 14px 0;
    border-bottom: 1px solid var(--border);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .label {
    flex: 1;
    font-weight: 500;
  }
  .state {
    font-size: var(--t12);
  }
  .bar {
    height: 4px;
    margin: 8px 0 6px;
    border-radius: 2px;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--border);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--fg);
    transition: width 300ms var(--ease);
  }
  .meta {
    font-size: var(--t12);
  }
  .error {
    margin: 6px 0 0;
    color: var(--danger);
    font-size: var(--t13);
  }
</style>
