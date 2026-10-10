<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  type Props = HTMLButtonAttributes & {
    label: string;
    active?: boolean;
    size?: 28 | 32 | 36;
    children?: Snippet;
  };

  let { label, active = false, size = 32, children, class: cls = '', ...rest }: Props = $props();
</script>

<button
  class="icon-btn {cls}"
  class:active
  style:width="{size}px"
  style:height="{size}px"
  aria-label={label}
  title={label}
  {...rest}
>
  {@render children?.()}
</button>

<style>
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    transition:
      background 120ms var(--ease),
      color 120ms var(--ease);
  }
  .icon-btn:hover:not(:disabled),
  .active {
    background: var(--surface);
    color: var(--fg);
  }
  .icon-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
