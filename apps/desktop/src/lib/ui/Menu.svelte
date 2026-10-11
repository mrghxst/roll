<script lang="ts" module>
  export interface MenuItem {
    label: string;
    hint?: string;
    checked?: boolean;
    disabled?: boolean;
    /** Indents subcategories. */
    depth?: number;
    run: () => void;
  }
</script>

<script lang="ts">
  import { Check } from '@lucide/svelte';
  import { tick, type Snippet } from 'svelte';

  type Props = {
    label: string;
    items: MenuItem[];
    trigger: Snippet<[{ open: boolean; toggle: () => void }]>;
    align?: 'left' | 'right';
  };

  let { label, items, trigger, align = 'right' }: Props = $props();
  let open = $state(false);
  let active = $state(0);
  let root: HTMLDivElement | undefined = $state();
  let list: HTMLDivElement | undefined = $state();

  async function toggle() {
    open = !open;
    if (open) {
      active = Math.max(0, items.findIndex((i) => !i.disabled));
      await tick();
      list?.focus();
    }
  }

  function choose(i: MenuItem | undefined) {
    if (!i || i.disabled) return;
    open = false;
    i.run();
  }

  function onkeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === 'Escape') {
      e.preventDefault();
      open = false;
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const d = e.key === 'ArrowDown' ? 1 : -1;
      for (let k = 1; k <= items.length; k++) {
        const j = (active + d * k + items.length) % items.length;
        if (!items[j]!.disabled) {
          active = j;
          break;
        }
      }
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      choose(items[active]);
    }
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }}
/>

<div class="menu-root" bind:this={root}>
  {@render trigger({ open, toggle })}
  {#if open}
    <div class="menu {align}" role="menu" aria-label={label} tabindex="-1" bind:this={list} {onkeydown}>
      {#each items as item, i (i)}
        <button
          role="menuitem"
          class:active={i === active}
          disabled={item.disabled}
          style:padding-left="{10 + (item.depth ?? 0) * 14}px"
          onpointermove={() => (active = i)}
          onclick={() => choose(item)}
        >
          <span class="check">{#if item.checked}<Check size={14} strokeWidth={2} />{/if}</span>
          <span class="label">{item.label}</span>
          {#if item.hint}<span class="hint mono">{item.hint}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .menu-root {
    position: relative;
    display: inline-flex;
  }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    z-index: 40;
    min-width: 200px;
    max-height: 60vh;
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--bg);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.14);
    outline: none;
    animation: menu-in 120ms var(--ease);
  }
  .right {
    right: 0;
  }
  .left {
    left: 0;
  }
  button {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 30px;
    padding-right: 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--fg);
    font-size: var(--t13);
    text-align: left;
    cursor: pointer;
  }
  button.active:not(:disabled) {
    background: var(--surface);
  }
  button:disabled {
    color: var(--muted);
    cursor: default;
  }
  .check {
    display: grid;
    place-items: center;
    width: 16px;
    flex: none;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  .hint {
    color: var(--muted);
    font-size: var(--t12);
  }
  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
</style>
