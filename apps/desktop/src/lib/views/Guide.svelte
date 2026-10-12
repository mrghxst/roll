<script lang="ts">
  import { Check, ArrowRight } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { BatchInfo } from '../ipc/bindings/BatchInfo';
  import { app, type View } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import { n } from '../util/format';

  interface Step {
    key: string;
    title: string;
    text: string;
    view: View;
    /** Done without a visit: nothing left to do there. */
    auto?: boolean;
  }

  let history = $state<BatchInfo[]>([]);
  $effect(() => {
    void app.historyRevision;
    void api
      .history(200)
      .then((h) => (history = h))
      .catch(() => undefined);
  });

  const applied = $derived(history.some((b) => b.kind === 'organize' && b.state !== 'planned'));
  const synced = $derived(history.some((b) => b.kind === 'immich'));
  const c = $derived(app.counts);

  const steps = $derived<Step[]>([
    {
      key: 'review',
      title: 'Check automatic decisions',
      text: c.review
        ? `${n(c.review)} items wait for a look, the most doubtful first. Keys 1–9 pick a category, Enter accepts.`
        : 'Nothing waits for a look.',
      view: 'review',
      auto: c.review === 0,
    },
    {
      key: 'discover',
      title: 'Sort groups of saved images',
      text: 'Images saved from the same app share a size and encoder. One decision sorts a whole group.',
      view: 'discover',
    },
    {
      key: 'dates',
      title: 'Fix dates',
      text: c.date_issues ? `${n(c.date_issues)} items have a doubtful date: camera clocks, time zones, missing dates.` : 'No doubtful dates.',
      view: 'dates',
      auto: c.date_issues === 0,
    },
    {
      key: 'places',
      title: 'Fix places',
      text: c.no_location ? `${n(c.no_location)} items have no place. Suggestions come from your other photos and location history.` : 'Every item has a place.',
      view: 'places',
      auto: c.no_location === 0,
    },
    {
      key: 'organize',
      title: 'Preview the folders',
      text: 'See where every sorted item would go, before anything moves.',
      view: 'organize',
    },
    {
      key: 'apply',
      title: 'Apply',
      text: 'Move the files and save dates, places and tags into them. Everything lands in History and can be undone.',
      view: 'organize',
      auto: applied,
    },
    {
      key: 'immich',
      title: 'First Immich sync',
      text: 'Send the chosen part to Immich, after checking the plan. Skip this if you don’t use Immich.',
      view: 'immich',
      auto: synced,
    },
  ]);

  const seen = $derived(new Set(app.settings?.guide_steps ?? []));
  const isDone = (s: Step) => !!s.auto || (s.key !== 'apply' && s.key !== 'immich' && seen.has(s.key)) || seen.has(`done:${s.key}`);
  const doneCount = $derived(steps.filter(isDone).length);

  async function mark(key: string) {
    const list = app.settings?.guide_steps ?? [];
    if (!list.includes(key)) await app.saveSettings({ guide_steps: [...list, key] });
  }

  async function open(s: Step) {
    await mark(s.key);
    app.go(s.view);
  }

  async function finish() {
    await app.saveSettings({ guide_done: true });
    toast.show('The guide is closed. “Get started” in the command palette (Ctrl K) brings it back.');
    app.go('timeline');
  }
</script>

<div class="guide">
  <header>
    <h1>Get started</h1>
    <p class="lead">
      From a dump to a sorted, reviewed archive in seven steps. Each opens the view for it; nothing changes on disk until you apply.
    </p>
    <p class="progress mono" aria-live="polite">{doneCount} of {steps.length} done</p>
  </header>
  <ol class="steps">
    {#each steps as s, i (s.key)}
      {@const done = isDone(s)}
      <li class:done data-testid="guide-{s.key}">
        <span class="num" aria-hidden="true">{#if done}<Check size={14} strokeWidth={2} />{:else}{i + 1}{/if}</span>
        <div class="body">
          <strong>{s.title}</strong>
          <p>{s.text}</p>
        </div>
        <div class="actions">
          <Button variant={done ? 'ghost' : 'secondary'} onclick={() => void open(s)}>Open <ArrowRight size={14} strokeWidth={1.5} /></Button>
          {#if !done}<Button variant="ghost" onclick={() => void mark(`done:${s.key}`)}>Mark as done</Button>{/if}
        </div>
      </li>
    {/each}
  </ol>
  <div class="foot">
    <Button variant={doneCount === steps.length ? 'primary' : 'ghost'} onclick={() => void finish()} data-testid="guide-close">Close the guide</Button>
  </div>
</div>

<style>
  .guide {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  .guide > * {
    max-width: 760px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 8px;
    color: var(--muted);
  }
  .progress {
    margin: 0 0 16px;
    font-size: var(--t12);
    color: var(--muted);
  }
  .steps {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }
  .steps li {
    display: grid;
    grid-template-columns: 28px 1fr auto;
    gap: 12px;
    align-items: start;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    font-size: var(--t13);
  }
  .num {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--border);
    font-size: var(--t12);
  }
  .done .num {
    background: var(--fg);
    color: var(--bg);
    border-color: var(--fg);
  }
  .done strong {
    color: var(--muted);
  }
  .body p {
    margin: 2px 0 0;
    color: var(--muted);
  }
  .body strong {
    font-weight: 500;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    margin-top: 16px;
  }
</style>
