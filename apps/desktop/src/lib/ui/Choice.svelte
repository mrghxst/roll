<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    selected,
    onselect,
    title,
    badge,
    children,
  }: { selected: boolean; onselect: () => void; title: string; badge?: string; children?: Snippet } = $props();
</script>

<button class="choice" role="radio" aria-checked={selected} onclick={onselect}>
  <span class="dot" aria-hidden="true"></span>
  <span class="body">
    <span class="title">{title}{#if badge}<span class="badge">{badge}</span>{/if}</span>
    {#if children}<ul>{@render children()}</ul>{/if}
  </span>
</button>

<style>
  .choice {
    display: flex;
    gap: 12px;
    width: 100%;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms var(--ease);
  }
  .choice:hover {
    border-color: var(--border-strong);
  }
  .choice[aria-checked='true'] {
    border-color: var(--fg);
    box-shadow: inset 0 0 0 1px var(--fg);
  }
  .dot {
    flex: none;
    width: 16px;
    height: 16px;
    margin-top: 2px;
    border: 1px solid var(--border-strong);
    border-radius: 50%;
  }
  .choice[aria-checked='true'] .dot {
    border: 5px solid var(--fg);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 500;
  }
  .badge {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    color: var(--muted);
    font-size: 11px;
    font-weight: 400;
  }
  ul {
    margin: 6px 0 0;
    padding-left: 16px;
    color: var(--muted);
    font-size: var(--t13);
  }
  ul :global(li) {
    margin-bottom: 2px;
  }
</style>
