<script lang="ts">
  import type { Snippet } from 'svelte';
  import { X } from '@lucide/svelte';
  import IconButton from './IconButton.svelte';

  type Props = {
    open: boolean;
    title: string;
    width?: number;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
  };

  let { open, title, width = 480, onclose, children, footer }: Props = $props();
  let dialog: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  });
</script>

<dialog
  bind:this={dialog}
  class="sheet"
  style:width="{width}px"
  aria-label={title}
  onclose={() => {
    if (open) onclose();
  }}
  onclick={(e) => {
    if (e.target === dialog) onclose();
  }}
>
  {#if open}
    <header>
      <h2>{title}</h2>
      <IconButton label="Close" size={28} onclick={onclose}><X size={16} strokeWidth={1.5} /></IconButton>
    </header>
    <div class="body">{@render children()}</div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  {/if}
</dialog>

<style>
  .sheet {
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--bg);
    color: var(--fg);
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.16);
    overflow: hidden;
    flex-direction: column;
  }
  .sheet[open] {
    display: flex;
    animation: sheet-in 180ms var(--ease);
  }
  .sheet::backdrop {
    background: var(--overlay);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 12px 12px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: var(--t16);
    font-weight: 600;
  }
  .body {
    padding: 20px;
    overflow: auto;
    font-size: var(--t14);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
  @keyframes sheet-in {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.98);
    }
  }
</style>
