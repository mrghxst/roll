<script lang="ts">
  import { Undo2 } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { BatchInfo } from '../ipc/bindings/BatchInfo';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import Kbd from '../ui/Kbd.svelte';
  import { n } from '../util/format';

  let list = $state<BatchInfo[]>([]);
  let loaded = $state(false);
  let limit = $state(100);

  $effect(() => {
    void app.historyRevision;
    void api.history(limit).then((h) => {
      list = h;
      loaded = true;
    });
  });

  const STATES: Record<string, string> = {
    applied: '',
    applying: 'Applying…',
    undoing: 'Undoing…',
    undone: 'Undone',
    failed: 'Not applied',
  };

  function when(ms: number) {
    const d = new Date(ms);
    const today = new Date();
    const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
    if (d.toDateString() === today.toDateString()) return `Today ${time}`;
    return `${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: d.getFullYear() === today.getFullYear() ? undefined : 'numeric' })} ${time}`;
  }

  async function undo(b: BatchInfo) {
    try {
      await api.undo(b.id);
      toast.show(b.kind === 'organize' ? `Undoing “${b.label}”. Files move back in the background.` : `Undid “${b.label}”`);
    } catch (e) {
      toast.error(e);
    }
  }

  const undoable = (b: BatchInfo) => b.state === 'applied' && b.kind !== 'empty_trash';
</script>

<div class="history">
  <header>
    <h1>History</h1>
    <p>Every change Roll made, newest first. Undo restores the previous values and moves files back. <Kbd keys="Ctrl+Z" /> undoes the latest.</p>
  </header>
  {#if loaded && list.length === 0}
    <EmptyState text="No changes yet. Edits, plans you apply and trash actions appear here." />
  {:else}
    <ol class="list" data-testid="history">
      {#each list as b (b.id)}
        <li class:undone={b.state === 'undone'}>
          <div class="main">
            <span class="label">{b.label}</span>
            <span class="meta">
              {when(b.applied_at ?? b.created_at)}
              · {n(b.ops)} {b.ops === 1 ? 'change' : 'changes'}
              {#if b.failed}· <span class="warn">{n(b.failed)} failed</span>{/if}
              {#if STATES[b.state]}· {STATES[b.state]}{/if}
            </span>
          </div>
          {#if undoable(b)}
            <Button variant="ghost" onclick={() => void undo(b)} aria-label="Undo {b.label}"><Undo2 size={14} strokeWidth={1.5} /> Undo</Button>
          {:else if b.kind === 'empty_trash'}
            <span class="hint">Restore from the Recycle Bin</span>
          {/if}
        </li>
      {/each}
    </ol>
    {#if list.length === limit}
      <div class="more"><Button variant="ghost" onclick={() => (limit += 100)}>Show older changes</Button></div>
    {/if}
  {/if}
</div>

<style>
  .history {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  header {
    max-width: 760px;
    margin-bottom: 16px;
  }
  h1 {
    margin: 0 0 6px;
    font-size: var(--t24);
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  header p {
    margin: 0;
    color: var(--muted);
    font-size: var(--t13);
  }
  .list {
    max-width: 760px;
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--border);
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
  }
  li.undone .label {
    color: var(--muted);
    text-decoration: line-through;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  .label {
    font-size: var(--t14);
  }
  .meta,
  .hint {
    color: var(--muted);
    font-size: var(--t12);
  }
  .warn {
    color: var(--danger);
  }
  .more {
    max-width: 760px;
    padding-top: 12px;
    text-align: center;
  }
</style>
