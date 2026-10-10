<script lang="ts">
  let { keys }: { keys: string } = $props();
</script>

<span class="kbd-group">
  {#each keys.split('+') as k, i (i)}<kbd>{k}</kbd>{/each}
</span>

<style>
  .kbd-group {
    display: inline-flex;
    gap: 2px;
  }
  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    background: var(--surface);
    color: var(--muted);
    font-family: var(--mono);
    font-size: 11px;
  }
</style>
