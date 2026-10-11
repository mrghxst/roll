<script lang="ts">
  import { Check, CircleAlert } from '@lucide/svelte';
  import { untrack } from 'svelte';
  import { api } from '../ipc/api';
  import type { AssetDetail } from '../ipc/bindings/AssetDetail';
  import type { Candidate } from '../ipc/bindings/Candidate';
  import type { DateFix } from '../ipc/bindings/DateFix';
  import type { DatePreview } from '../ipc/bindings/DatePreview';
  import type { DateSet } from '../ipc/bindings/DateSet';
  import { app, mergeQuery } from '../state/app.svelte';
  import { undoLast } from '../state/edits';

  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import EmptyState from '../ui/EmptyState.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Switch from '../ui/Switch.svelte';
  import { n } from '../util/format';
  import { OFFSETS, shortLocal } from '../util/zones';

  type Mode = 'shift' | 'zone' | 'drift';

  let sets = $state<DateSet[]>([]);
  let loaded = $state(false);
  let key = $state<string | null>(null);
  let mode = $state<Mode>('shift');
  // References: the item and its true date and zone.
  let ref1 = $state<{ d: AssetDetail; local: string; offset: string } | null>(null);
  let ref2 = $state<{ d: AssetDetail; local: string; offset: string } | null>(null);
  let zone = $state('');
  let wholeCamera = $state(false);
  let preview = $state<DatePreview | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let phones = $state<number[]>([]);
  let candidates = $state<Candidate[]>([]);

  $effect(() => {
    void app.revision;
    void api.dateSets().then((s) => {
      sets = s;
      loaded = true;
      // Open the most suspect set on arrival; keep the current one after edits.
      untrack(() => {
        if (!key || !s.some((x) => x.key === key)) key = s[0]?.key ?? null;
      });
    });
  });

  const set = $derived(sets.find((s) => s.key === key) ?? null);

  // A new set: reset the form.
  $effect(() => {
    void key;
    untrack(() => {
      ref1 = null;
      ref2 = null;
      zone = '';
      wholeCamera = false;
      preview = null;
      error = null;
      mode = 'shift';
      candidates = [];
    });
  });

  $effect(() => {
    const s = set;
    if (s?.kind === 'missing') void api.dateCandidates(s.ids.slice(0, 300)).then((c) => (candidates = c));
  });

  const input = (d: AssetDetail) => (d.taken_local ?? '').slice(0, 19);

  async function pick(id: number) {
    const d = await api.assetDetail(id);
    if (!d) return;
    const r = { d, local: input(d), offset: d.tz_offset != null ? String(d.tz_offset) : '' };
    if (mode === 'drift' && ref1 && ref1.d.id !== id) ref2 = r;
    else ref1 = r;
  }

  /** Use a phone photo of the same moment: its date becomes the truth. */
  async function usePhone(id: number) {
    const d = await api.assetDetail(id);
    if (!d || !ref1) return;
    ref1 = { ...ref1, local: input(d), offset: d.tz_offset != null ? String(d.tz_offset) : ref1.offset };
  }

  // Photos from other devices around the date the owner typed.
  let phoneTimer = 0;
  $effect(() => {
    const r = ref1;
    const s = set;
    clearTimeout(phoneTimer);
    if (!r || !s || mode === 'zone') {
      phones = [];
      return;
    }
    phoneTimer = window.setTimeout(async () => {
      const t = Date.parse(`${r.local}Z`);
      if (Number.isNaN(t)) return;
      const ids = await api.queryIds(mergeQuery({ from_utc: t - 30 * 3_600_000, to_utc: t + 30 * 3_600_000 }));
      const own = new Set(s.ids);
      phones = ids.filter((id) => !own.has(id)).slice(0, 40);
    }, 250);
  });

  const fix = $derived.by<DateFix | null>(() => {
    const s = set;
    if (!s || s.kind !== 'camera') return null;
    const ref = (r: typeof ref1) => (r ? { asset_id: r.d.id, local: r.local.length === 16 ? `${r.local}:00` : r.local, offset_min: r.offset === '' ? null : Number(r.offset) } : null);
    const base: DateFix = {
      ids: s.ids,
      mode,
      reference: null,
      second: null,
      offset_min: null,
      amount: null,
      exact: null,
      camera_from: wholeCamera ? s.from_local : null,
      camera_to: wholeCamera ? s.to_local : null,
    };
    if (mode === 'zone') return zone === '' ? null : { ...base, offset_min: Number(zone) };
    if (!ref1) return null;
    if (mode === 'drift') return ref2 ? { ...base, reference: ref(ref1), second: ref(ref2) } : null;
    return { ...base, reference: ref(ref1) };
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
        preview = null;
        error = String(e).replace(/^Error: /, '');
      }
    }, 150);
  });

  async function apply(f: DateFix | null) {
    if (!f) return;
    busy = true;
    try {
      const folders = preview?.folder_changes ?? 0;
      const done = await api.dateApply(f);
      toast.show(`Fixed the date of ${n(done)} ${done === 1 ? 'item' : 'items'}${folders ? `; ${n(folders)} belong in another year folder now` : ''}`, {
        action: folders ? { label: 'Organize', run: () => app.go('organize') } : { label: 'Undo', run: () => void undoLast() },
      });
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  async function dismiss(s: DateSet) {
    await api.dismiss('date', s.key);
    sets = sets.filter((x) => x.key !== s.key);
    key = sets[0]?.key ?? null;
  }

  const suggestedFix = $derived<DateFix | null>(
    set?.kind === 'missing'
      ? { ids: set.ids, mode: 'suggested', reference: null, second: null, offset_min: null, amount: null, exact: null, camera_from: null, camera_to: null }
      : null,
  );
  const SOURCE: Record<string, string> = { filename: 'File name', folder: 'Folder', sequence: 'Neighbouring files', file_time: 'File time' };
</script>

<div class="dates">
  <nav aria-label="Sets to check">
    <h1>Dates</h1>
    {#if loaded && !sets.length}
      <p class="muted">Nothing to check.</p>
    {/if}
    {#each sets as s (s.key)}
      <button class="set" class:active={s.key === key} onclick={() => (key = s.key)} data-testid="date-set">
        <span class="name">
          {#if s.reasons.length}<CircleAlert size={14} strokeWidth={1.5} />{/if}
          {s.label}
        </span>
        <span class="muted">{n(s.count)} · {shortLocal(s.from_local).slice(0, -6)} – {shortLocal(s.to_local).slice(0, -6)}</span>
      </button>
    {/each}
  </nav>

  <section class="detail">
    {#if !set}
      <EmptyState text={loaded ? 'No camera sets or undated items. Select items anywhere and press D to change their dates.' : 'Looking for date problems…'} />
    {:else}
      <header>
        <h2>{set.label}</h2>
        <p class="muted">{n(set.count)} items, {shortLocal(set.from_local)} – {shortLocal(set.to_local)}{set.has_offset ? '' : ', no time zone in the files'}</p>
        {#each set.reasons as r (r)}<p class="reason">{r}</p>{/each}
      </header>

      {#if set.kind === 'camera'}
        <Segmented
          label="Fix"
          options={[
            { value: 'shift', label: 'Shift to a reference' },
            { value: 'zone', label: 'Set the time zone' },
            { value: 'drift', label: 'Correct drift' },
          ]}
          value={mode}
          onchange={(v) => (mode = v as Mode)}
        />

        {#if mode === 'zone'}
          <div class="field">
            <span>Time zone the camera clock was set to</span>
            <select bind:value={zone} aria-label="Time zone">
              <option value="">Choose…</option>
              {#each OFFSETS as o (o.value)}<option value={String(o.value)}>{o.label}</option>{/each}
            </select>
            <span class="hint">Keeps the clock times and states the zone, so the photos sort correctly against other devices.</span>
          </div>
        {:else}
          <p class="step">
            {mode === 'drift' && ref1 ? 'Pick a second reference, as far from the first as possible.' : 'Pick a photo you know the true time of.'}
          </p>
          <div class="strip" role="listbox" aria-label="Photos in this set">
            {#each set.ids.slice(0, 60) as id (id)}
              <button
                class="thumb"
                class:picked={ref1?.d.id === id || ref2?.d.id === id}
                onclick={() => void pick(id)}
                role="option"
                aria-selected={ref1?.d.id === id || ref2?.d.id === id}
                data-testid="set-thumb"><img src={api.thumbUrl(id, 1)} alt="" loading="lazy" /></button
              >
            {/each}
          </div>

          {#each [ref1, ref2] as r, i (i)}
            {#if r}
              <div class="ref">
                <img src={api.thumbUrl(r.d.id, r.d.thumb_v)} alt="" />
                <div class="refbody">
                  <span class="muted">{i === 0 ? 'Reference' : 'Second reference'}: {r.d.name}, the camera says {shortLocal(r.d.taken_local)}</span>
                  <div class="row">
                    <label>
                      <span>True date and time</span>
                      <input
                        type="datetime-local"
                        step="1"
                        value={r.local}
                        oninput={(e) => {
                          const v = (e.currentTarget as HTMLInputElement).value;
                          if (i === 0 && ref1) ref1 = { ...ref1, local: v };
                          else if (ref2) ref2 = { ...ref2, local: v };
                        }}
                        aria-label="True date and time"
                        data-testid="true-date"
                      />
                    </label>
                    <label>
                      <span>Time zone</span>
                      <select
                        value={r.offset}
                        onchange={(e) => {
                          const v = (e.currentTarget as HTMLSelectElement).value;
                          if (i === 0 && ref1) ref1 = { ...ref1, offset: v };
                          else if (ref2) ref2 = { ...ref2, offset: v };
                        }}
                        aria-label="Time zone"
                        data-testid="true-zone"
                      >
                        <option value="">Not stated</option>
                        {#each OFFSETS as o (o.value)}<option value={String(o.value)}>{o.label}</option>{/each}
                      </select>
                    </label>
                  </div>
                </div>
              </div>
            {/if}
          {/each}

          {#if ref1 && phones.length && mode === 'shift'}
            <p class="step">Or click a photo from another device taken at the same moment; its time and zone become the truth.</p>
            <div class="strip small">
              {#each phones as id (id)}
                <button class="thumb" onclick={() => void usePhone(id)} aria-label="Use this photo's time"><img src={api.thumbUrl(id, 1)} alt="" loading="lazy" /></button>
              {/each}
            </div>
          {/if}
        {/if}

        <label class="toggle">
          <Switch label="Fix every photo from this camera in this range" bind:checked={wholeCamera} />
          <span>Also fix every photo from this camera between {shortLocal(set.from_local)} and {shortLocal(set.to_local)}, in other folders too</span>
        </label>

        {#if error}
          <p class="error" role="alert">{error}</p>
        {:else if preview}
          <div class="preview" data-testid="date-preview">
            <p><strong>{n(preview.count)} items, {preview.label}</strong></p>
            <p class="muted">{shortLocal(preview.before_from)} – {shortLocal(preview.before_to)} becomes {shortLocal(preview.after_from)} – {shortLocal(preview.after_to)}</p>
            {#if preview.year_changes}<p class="muted">{n(preview.year_changes)} items change year{preview.folder_changes ? `, and ${n(preview.folder_changes)} sorted ones move to another year folder when you organize` : ''}.</p>{/if}
            <table>
              <thead><tr><th></th><th>Before</th><th>After</th></tr></thead>
              <tbody>
                {#each preview.samples as s (s.id)}
                  <tr><td><img src={api.thumbUrl(s.id, 1)} alt="" /></td><td class="mono">{shortLocal(s.before)}</td><td class="mono">{shortLocal(s.after)}</td></tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}

        <div class="actions">
          <Button variant="ghost" onclick={() => void dismiss(set)}>Dates look right</Button>
          <Button variant="primary" size="md" disabled={!fix || !preview || busy} onclick={() => void apply(fix)} data-testid="apply-dates">
            <Check size={16} strokeWidth={2} /> Fix {preview ? n(preview.count) : ''} dates
          </Button>
        </div>
      {:else}
        <table class="cands">
          <thead><tr><th></th><th>Suggested</th><th>From</th><th>Why</th></tr></thead>
          <tbody>
            {#each candidates as c (c.id)}
              <tr>
                <td><img src={api.thumbUrl(c.id, 1)} alt="" /></td>
                <td class="mono">{shortLocal(c.local)}</td>
                <td>{SOURCE[c.source] ?? c.source}</td>
                <td class="muted">{c.why}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <div class="actions">
          <Button onclick={() => set && (app.dating = set.ids)}>Set a date for all…</Button>
          <Button variant="primary" disabled={!candidates.length || busy} onclick={() => void apply(suggestedFix)}>Use the suggested dates</Button>
        </div>
      {/if}
    {/if}
  </section>
</div>

<style>
  .dates {
    display: flex;
    height: 100%;
  }
  nav {
    width: 280px;
    flex: none;
    overflow-y: auto;
    padding: 20px 12px;
    border-right: 1px solid var(--border);
  }
  h1 {
    margin: 0 8px 12px;
    font-size: var(--t24);
    font-weight: 600;
  }
  h2 {
    margin: 0 0 4px;
    font-size: var(--t16);
    font-weight: 600;
  }
  .set {
    display: grid;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: var(--fg);
    text-align: left;
    cursor: pointer;
  }
  .set:hover {
    background: var(--surface);
  }
  .set.active {
    background: var(--surface);
    box-shadow: 0 0 0 1px var(--border);
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t13);
  }
  .muted,
  .hint {
    color: var(--muted);
    font-size: var(--t12);
  }
  .detail {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 24px 32px 48px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    max-width: 980px;
  }
  header p {
    margin: 0;
  }
  .reason {
    margin-top: 6px !important;
    font-size: var(--t13);
  }
  .step {
    margin: 0;
    font-size: var(--t13);
  }
  .strip {
    flex: none;
    display: flex;
    gap: 4px;
    overflow-x: auto;
    padding-bottom: 4px;
  }
  .thumb {
    flex: none;
    width: 76px;
    height: 76px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: var(--r-1);
    background: var(--surface);
    cursor: pointer;
  }
  .small .thumb {
    width: 60px;
    height: 60px;
  }
  .thumb.picked {
    border-color: var(--fg);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 2px;
  }
  .ref {
    flex: none;
    display: flex;
    gap: 12px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
  }
  .ref > img {
    width: 96px;
    height: 96px;
    object-fit: cover;
    border-radius: var(--r-1);
  }
  .refbody {
    display: grid;
    gap: 8px;
    align-content: start;
  }
  .row {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }
  label {
    display: grid;
    gap: 4px;
    font-size: var(--t12);
    color: var(--muted);
  }
  .field {
    display: grid;
    gap: 6px;
    max-width: 420px;
    font-size: var(--t13);
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
  .toggle {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    color: var(--fg);
    font-size: var(--t13);
  }
  .preview {
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    font-size: var(--t13);
  }
  .preview p {
    margin: 0 0 4px;
  }
  table {
    margin-top: 8px;
    border-collapse: collapse;
    font-size: var(--t12);
  }
  th {
    padding: 4px 12px 4px 0;
    color: var(--muted);
    font-weight: 500;
    text-align: left;
  }
  td {
    padding: 2px 12px 2px 0;
  }
  td img {
    width: 36px;
    height: 36px;
    object-fit: cover;
    border-radius: 3px;
    vertical-align: middle;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--t13);
  }
  .actions {
    position: sticky;
    bottom: -48px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 0;
    background: var(--bg);
    border-top: 1px solid var(--border);
  }
</style>
