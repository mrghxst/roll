<script lang="ts">
  import { X } from '@lucide/svelte';
  import { toast } from '../state/toast.svelte';
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each toast.items as t (t.id)}
    <div class="toast" class:error={t.error} data-testid="toast">
      <span>{t.text}</span>
      {#if t.action}
        <button
          class="action"
          onclick={() => {
            toast.dismiss(t.id);
            t.action?.run();
          }}>{t.action.label}</button
        >
      {/if}
      <button class="close" aria-label="Dismiss" onclick={() => toast.dismiss(t.id)}><X size={14} strokeWidth={1.5} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    left: 50%;
    bottom: 20px;
    transform: translateX(-50%);
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 40px;
    max-width: min(560px, calc(100vw - 32px));
    padding: 0 6px 0 16px;
    border-radius: var(--r-sheet);
    background: var(--fg);
    color: var(--bg);
    font-size: var(--t13);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
    pointer-events: auto;
    animation: toast-in 160ms var(--ease);
  }
  .toast.error {
    background: var(--danger);
    color: #fff;
  }
  .action {
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: color-mix(in srgb, var(--bg) 18%, transparent);
    color: inherit;
    font-weight: 600;
    font-size: var(--t13);
    cursor: pointer;
  }
  .action:hover {
    background: color-mix(in srgb, var(--bg) 28%, transparent);
  }
  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
  }
  .close:hover {
    opacity: 1;
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
