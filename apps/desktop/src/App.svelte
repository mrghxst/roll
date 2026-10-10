<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api } from './lib/ipc/api';
  import Shell from './lib/shell/Shell.svelte';
  import { app } from './lib/state/app.svelte';
  import FirstRun from './lib/views/FirstRun.svelte';

  onMount(async () => {
    await app.init();
    await tick();
    // The window starts hidden so it never flashes white before the theme applies.
    requestAnimationFrame(() => void api.showWindow());
  });
</script>

{#if app.error}
  <main class="fatal" role="alert">
    <h1>Roll could not open the library</h1>
    <p>{app.error}</p>
    <p class="muted">Close other Roll windows and start Roll again. If this keeps happening, the log in the app data folder says more.</p>
  </main>
{:else if app.ready && app.settings}
  {#if !app.settings.first_run_done}
    <FirstRun />
  {:else}
    <Shell />
  {/if}
{/if}

<style>
  .fatal {
    max-width: 560px;
    margin: 15vh auto 0;
    padding: 24px;
  }
  .fatal h1 {
    font-size: var(--t24);
    margin: 0 0 12px;
  }
  .fatal p {
    margin: 0 0 12px;
  }
</style>
