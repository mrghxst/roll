<script lang="ts">
  import { FileDown, FolderPlus, Play } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { PlanSummary } from '../ipc/bindings/PlanSummary';
  import type { TileInfo } from '../ipc/bindings/TileInfo';
  import { app } from '../state/app.svelte';
  import { selection } from '../state/selection.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Switch from '../ui/Switch.svelte';
  import { n } from '../util/format';

  const s = $derived(app.settings!);
  // Captured on open: the selection belongs to the view the owner came from.
  const selected = selection.list();
  let scope = $state<'all' | 'selected'>(selected.length ? 'selected' : 'all');
  let writeMetadata = $state(true);
  let plan = $state<PlanSummary | null>(null);
  let planRevision = $state(-1);
  let busy = $state(false);
  let tiles = $state(new Map<number, TileInfo>());

  const stale = $derived(plan != null && planRevision !== app.revision);
  const nothing = $derived(plan != null && plan.files === 0 && plan.writes === 0 && plan.new_folders === 0);
  const applying = $derived(app.activeJobs.some((j) => j.kind === 'apply' || j.kind === 'undo'));

  async function preview() {
    busy = true;
    try {
      plan = await api.plan({
        rename: s.rename_files,
        rename_template: s.rename_template,
        ids: scope === 'selected' ? selected : [],
        write_metadata: writeMetadata,
      });
      planRevision = app.revision;
      const ids = plan.folders.flatMap((f) => f.samples);
      const t = ids.length ? await api.tiles(ids) : [];
      tiles = new Map(t.map((x) => [x.id, x]));
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  async function apply() {
    if (!plan) return;
    busy = true;
    try {
      await api.applyPlan(plan.batch_id);
      toast.show('Applying the plan. Progress is in Activity, and History can undo it.', {
        action: { label: 'Show activity', run: () => app.go('activity') },
      });
      plan = null;
    } catch (e) {
      toast.error(e);
    } finally {
      busy = false;
    }
  }

  async function exportCsv() {
    if (!plan) return;
    try {
      if (await api.savePlanCsv(plan.batch_id)) toast.show('Saved the plan as CSV');
    } catch (e) {
      toast.error(e);
    }
  }

  const stats = $derived(
    plan
      ? [
          ['Moved', plan.moved],
          ['Renamed', plan.renamed],
          ['Unchanged', plan.unchanged],
          ['Without a category', plan.undecided],
          ['Private, not reviewed', plan.private_held],
          ['Metadata saves', plan.writes],
          ['New folders', plan.new_folders],
        ]
      : [],
  );
  const rootPath = (id: number) => app.roots.find((r) => r.id === id)?.path ?? '';
</script>

<div class="organize">
  <header>
    <h1>Organize</h1>
    <p class="lead">
      Moves items into folders by category and saves edited dates, places, tags and ratings into the files. Nothing changes on disk until you
      apply the plan, and History can undo it.
    </p>
  </header>

  <section class="options" aria-label="Plan options">
    <div class="field">
      <span>Items</span>
      <Segmented
        label="Items"
        options={[
          { value: 'all', label: 'Whole library' },
          { value: 'selected', label: selected.length ? `Selected (${n(selected.length)})` : 'Selected' },
        ]}
        value={scope}
        onchange={(v) => (scope = v as 'all' | 'selected')}
      />
    </div>
    <label class="toggle">
      <Switch label="Save changes into the files" bind:checked={writeMetadata} />
      <span>
        Save changes into the files
        <span class="muted">{app.unsaved ? `${n(app.unsaved)} items have unsaved changes.` : 'No unsaved changes.'}</span>
      </span>
    </label>
    <label class="toggle">
      <Switch label="Rename files" checked={s.rename_files} onchange={(v) => void app.saveSettings({ rename_files: v })} />
      <span>Rename files <span class="muted">Names are kept unless this is on.</span></span>
    </label>
    {#if s.rename_files}
      <div class="field indent">
        <span>Name template</span>
        <Input
          mono
          value={s.rename_template}
          onchange={(e) => void app.saveSettings({ rename_template: (e.currentTarget as HTMLInputElement).value })}
          aria-label="Name template"
        />
        <span class="hint">Tokens: {'{date}'} {'{time}'} {'{camera}'} {'{year}'} {'{month}'}. A number is added when names collide.</span>
      </div>
    {/if}
    <div class="row">
      <Button variant={plan && !stale ? 'secondary' : 'primary'} disabled={busy || (scope === 'selected' && !selected.length)} onclick={preview} data-testid="preview-plan">
        {plan ? 'Preview again' : 'Preview plan'}
      </Button>
    </div>
  </section>

  {#if plan}
    <section class="plan" aria-label="Plan preview" data-testid="plan">
      {#if stale}
        <p class="notice" role="status">The library changed since this preview. Preview again before applying.</p>
      {/if}
      <dl class="stats">
        {#each stats as [label, value] (label)}
          <div><dt>{label}</dt><dd class="mono">{n(Number(value))}</dd></div>
        {/each}
      </dl>
      {#if plan.undecided}
        <p class="muted small">Items without a category stay where they are. Review them, then preview again.</p>
      {/if}
      {#if plan.private_held}
        <p class="muted small">
          Private items move only after you have looked at them. Open them from the Private filter, set their category, then preview again.
        </p>
      {/if}

      {#if plan.conflicts.length}
        <h2>Name conflicts ({n(plan.conflicts.length)})</h2>
        <p class="muted small">Existing files are never overwritten. These items get a number instead.</p>
        <ul class="conflicts">
          {#each plan.conflicts.slice(0, 50) as c (c.asset_id)}
            <li class="mono"><span class="muted">{c.wanted}</span> → {c.got}</li>
          {/each}
        </ul>
      {/if}

      {#if plan.folders.length}
        <h2>Target folders</h2>
        <ul class="folders">
          {#each plan.folders as f (`${f.root_id}:${f.path}`)}
            <li>
              <div class="fhead">
                <span class="mono fpath" title={`${rootPath(f.root_id)}\\${f.path}`}>{f.path || 'Library folder'}</span>
                {#if f.new}<span class="badge"><FolderPlus size={12} strokeWidth={1.5} /> new</span>{/if}
                <span class="mono muted">{n(f.count)} {f.count === 1 ? 'item' : 'items'}</span>
              </div>
              <div class="samples">
                {#each f.samples as id (id)}
                  {@const t = tiles.get(id)}
                  <img src={api.thumbUrl(id, t?.thumb_v ?? 1)} alt={t?.name ?? ''} title={t?.name} loading="lazy" />
                {/each}
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <div class="actions">
        <Button variant="ghost" onclick={exportCsv} disabled={nothing}><FileDown size={16} strokeWidth={1.5} /> Export as CSV…</Button>
        <Button variant="primary" size="md" disabled={busy || stale || nothing || applying} onclick={apply} data-testid="apply-plan">
          <Play size={16} strokeWidth={1.5} />
          {nothing ? 'Nothing to change' : applying ? 'Another plan is running' : 'Apply plan'}
        </Button>
      </div>
    </section>
  {/if}
</div>

<style>
  .organize {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 64px;
  }
  header,
  section {
    max-width: 880px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  h2 {
    margin: 24px 0 6px;
    font-size: var(--t14);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 20px;
    color: var(--muted);
    max-width: 72ch;
  }
  .options {
    display: grid;
    gap: 14px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--surface);
  }
  .field {
    display: grid;
    gap: 6px;
    font-size: var(--t13);
  }
  .indent {
    margin-left: 44px;
    max-width: 420px;
  }
  .toggle {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    font-size: var(--t13);
  }
  .toggle > span {
    display: grid;
    gap: 2px;
  }
  .muted {
    color: var(--muted);
  }
  .small,
  .hint {
    font-size: var(--t12);
  }
  .hint {
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: 8px;
  }
  .plan {
    margin-top: 24px;
  }
  .notice {
    padding: 8px 12px;
    border: 1px solid var(--border-strong);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 1px;
    margin: 0;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--border);
    overflow: hidden;
  }
  .stats div {
    padding: 12px 14px;
    background: var(--bg);
  }
  dt {
    color: var(--muted);
    font-size: var(--t12);
  }
  dd {
    margin: 4px 0 0;
    font-size: var(--t16);
  }
  .conflicts {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    font-size: var(--t12);
    display: grid;
    gap: 4px;
  }
  .folders {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
  }
  .folders li {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }
  .folders li:last-child {
    border-bottom: 0;
  }
  .fhead {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--t13);
  }
  .fpath {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    color: var(--muted);
    font-size: var(--t12);
  }
  .samples {
    display: flex;
    gap: 4px;
    margin-top: 8px;
    overflow: hidden;
  }
  .samples img {
    width: 52px;
    height: 52px;
    flex: none;
    object-fit: cover;
    border-radius: var(--r-1);
    background: var(--surface);
  }
  .actions {
    position: sticky;
    bottom: 0;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
    padding: 12px 0;
    background: var(--bg);
    border-top: 1px solid var(--border);
  }
</style>
