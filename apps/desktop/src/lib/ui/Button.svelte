<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = HTMLButtonAttributes & {
    variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
    size?: 'sm' | 'md';
    children?: Snippet;
  };

  let { variant = 'secondary', size = 'sm', children, class: cls = '', ...rest }: Props = $props();
</script>

<button class="btn {variant} {size} {cls}" {...rest}>
  {@render children?.()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border-radius: var(--r-btn);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t13);
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
    transition:
      background 120ms var(--ease),
      border-color 120ms var(--ease),
      opacity 120ms var(--ease);
  }
  .md {
    height: 36px;
    padding: 0 16px;
    font-size: var(--t14);
  }
  .secondary:hover:not(:disabled) {
    background: var(--surface);
    border-color: var(--border-strong);
  }
  .primary {
    background: var(--fg);
    color: var(--bg);
    border-color: var(--fg);
  }
  .primary:hover:not(:disabled) {
    opacity: 0.85;
  }
  .ghost {
    border-color: transparent;
    background: transparent;
  }
  .ghost:hover:not(:disabled) {
    background: var(--surface);
  }
  .danger {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
  .danger:hover:not(:disabled) {
    opacity: 0.85;
  }
  .btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
