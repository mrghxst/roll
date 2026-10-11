<script lang="ts">
  import { api } from '../ipc/api';
  import type { DateFix } from '../ipc/bindings/DateFix';
  import type { DatePreview } from '../ipc/bindings/DatePreview';
  import { app } from '../state/app.svelte';
  import { undoLast } from '../state/edits';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Sheet from '../ui/Sheet.svelte';
  import { n } from '../util/format';
  import { OFFSETS, shortLocal } from '../util/zones';

  let mode = $state<'amount' | 'exact'>('amount');
  let sign = $state<1 | -1>(1);
  let amount = $state({ years: 0, days: 0, hours: 0, minutes: 0, seconds: 0 });
  let exact = $state('');
  let offset = $state<string>('');
  let preview = $state<DatePreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  const ids = $derived(app.dating);

  $effect(() => {
    if (ids) {
      mode = 'amount';
      sign = 1;
      amount = { years: 0, days: 0, hours: 0, minutes: 0, seconds: 0 };
      exact = '';
      offset = '';
      preview = null;
    }
  });

  const fix = $derived.by<DateFix | null>(() => {
    if (!ids) return null;
    const base = { ids, reference: null, second: null, offset_min: null, amount: null, exact: null, camera_from: null, camera_to: null };
    if (mode === 'amount') {
      const a = Object.fromEntries(Object.entries(amount).map(([k, v]) => [k, Math.trunc(Number(v) || 0) * sign])) as typeof amount;
      if (Object.values(a).every((v) => v === 0)) return null;
      return { ...base, mode: 'amount', amount: a };
    }
    if (!exact) return null;
    const local = exact.length === 16 ? `${exact}:00` : exact;
    return { ...base, mode: 'exact', exact: { asset_id: 0, local, offset_min: offset === '' ? null : Number(offset) } };
  });

  let timer = 0;
  $effect(() => {
    const f = fix;
    clearTimeout(timer);
    if (!f) {
      preview = null;
      return;
    }
    timer = window.setTimeout(async () => {
      try {
        preview = await api.datePreview(f);
        error = null;
      } catch (e) {
        error = String(e).replace(/^Error: /, '');
      }
    }, 150);
  });

  async function apply() {
    if (!fix) return;
    busy = true;
    try {
      const done = await api.dateApply(fix);
      app.dating = null;
      toast.show(`Changed the date of ${n(done)} ${done === 1 ? 'item' : 'items'}`, { action: { label: 'Undo', run: () => void undoLast() } });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }
</script>

<Sheet open={ids != null} title="Change date" width={520} onclose={() => (app.dating = null)}>
  <Segmented
    label="How"
    options={[
      { value: 'amount', label: 'Shift by an amount' },
      { value: 'exact', label: 'Set a date' },
    ]}
    value={mode}
    onchange={(v) => (mode = v as 'amount' | 'exact')}
  />
  {#if mode === 'amount'}
    <div class="amount">
      <Segmented
        label="Direction"
        options={[
          { value: '1', label: 'Later' },
          { value: '-1', label: 'Earlier' },
        ]}
        value={String(sign)}
        onchange={(v) => (sign = Number(v) as 1 | -1)}
      />
      {#each [['years', 'Years'], ['days', 'Days'], ['hours', 'Hours'], ['minutes', 'Minutes'], ['seconds', 'Seconds']] as [k, label] (k)}
        <label>
          <span>{label}</span>
          <input type="number" min="0" bind:value={amount[k as keyof typeof amount]} aria-label={label} data-testid="amount-{k}" />
        </label>
      {/each}
    </div>
  {:else}
    <div class="exact">
      <label><span>Date and time</span><input type="datetime-local" step="1" bind:value={exact} aria-label="Date and time" /></label>
      <label>
        <span>Time zone</span>
        <select bind:value={offset} aria-label="Time zone">
          <option value="">Not stated</option>
          {#each OFFSETS as o (o.value)}<option value={String(o.value)}>{o.label}</option>{/each}
        </select>
      </label>
    </div>
  {/if}

  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if preview}
    <div class="preview" data-testid="date-preview">
      <p><strong>{n(preview.count)} {preview.count === 1 ? 'item' : 'items'}, {preview.label}</strong></p>
      <p class="muted">{shortLocal(preview.before_from)} – {shortLocal(preview.before_to)} becomes {shortLocal(preview.after_from)} – {shortLocal(preview.after_to)}</p>
      {#if preview.folder_changes}
        <p class="muted">{n(preview.folder_changes)} sorted items change year; Organize will offer to move them to their new year folder.</p>
      {/if}
    </div>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (app.dating = null)}>Cancel</Button>
    <Button variant="primary" disabled={!fix || !preview || busy || !!error} onclick={apply} data-testid="apply-date">Change {preview ? n(preview.count) : ''} dates</Button>
  {/snippet}
</Sheet>

<style>
  .amount,
  .exact {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 10px;
    margin-top: 14px;
  }
  label {
    display: grid;
    gap: 4px;
    font-size: var(--t12);
    color: var(--muted);
  }
  input,
  select {
    height: 32px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    font-size: var(--t13);
  }
  input[type='number'] {
    width: 72px;
  }
  .preview {
    margin-top: 16px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .preview p {
    margin: 0 0 4px;
  }
  .muted {
    color: var(--muted);
  }
  .error {
    margin-top: 12px;
    color: var(--danger);
    font-size: var(--t13);
  }
</style>
