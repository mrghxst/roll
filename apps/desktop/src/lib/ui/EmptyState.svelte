<script lang="ts">
  import type { Snippet } from 'svelte';

  let { text, action }: { text: string; action?: Snippet } = $props();
</script>

<div class="empty">
  <p>{text}</p>
  {#if action}{@render action()}{/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    height: 100%;
    min-height: 240px;
    padding: 32px;
    text-align: center;
  }
  p {
    margin: 0;
    max-width: 420px;
    color: var(--muted);
    font-size: var(--t14);
  }
</style>
