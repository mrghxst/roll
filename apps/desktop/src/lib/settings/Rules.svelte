<script lang="ts">
  import { Pencil, Plus, Trash2, X } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { api } from '../ipc/api';
  import type { Condition } from '../ipc/bindings/Condition';
  import type { Op } from '../ipc/bindings/Op';
  import type { Rule } from '../ipc/bindings/Rule';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Input from '../ui/Input.svelte';
  import Switch from '../ui/Switch.svelte';

  type Kind = 'text' | 'number' | 'flag';
  const FIELDS: [string, string, Kind][] = [
    ['name', 'File name', 'text'],
    ['path', 'Path in the library', 'text'],
    ['ext', 'Extension', 'text'],
    ['format', 'Format', 'text'],
    ['make', 'Camera make', 'text'],
    ['model', 'Camera model', 'text'],
    ['software', 'Software', 'text'],
    ['kind', 'Photo or video', 'text'],
    ['screenshot', 'Screenshot marker', 'flag'],
    ['has_camera', 'Camera make present', 'flag'],
    ['width', 'Width (px)', 'number'],
    ['height', 'Height (px)', 'number'],
    ['long_side', 'Long side (px)', 'number'],
    ['aspect', 'Aspect ratio', 'number'],
    ['duration_ms', 'Duration (ms)', 'number'],
    ['size', 'File size (bytes)', 'number'],
    ['broken', 'Broken file', 'flag'],
    ['junk', 'Looks like junk', 'flag'],
  ];
  const OPS: Record<Kind, [Op, string][]> = {
    text: [
      ['eq', 'is'],
      ['ne', 'is not'],
      ['regex', 'matches the pattern'],
      ['in', 'is one of'],
      ['exists', 'is present'],
      ['missing', 'is missing'],
    ],
    number: [
      ['lt', 'is less than'],
      ['gt', 'is more than'],
      ['eq', 'is'],
      ['exists', 'is present'],
      ['missing', 'is missing'],
    ],
    flag: [
      ['exists', 'is set'],
      ['missing', 'is not set'],
    ],
  };
  const fieldKind = (f: string): Kind => FIELDS.find((x) => x[0] === f)?.[2] ?? 'text';
  const fieldLabel = (f: string) => FIELDS.find((x) => x[0] === f)?.[1] ?? f;
  const opLabel = (c: Condition) => OPS[fieldKind(c.field)].find((o) => o[0] === c.op)?.[1] ?? c.op;
  const noValue = (op: Op) => op === 'exists' || op === 'missing';

  interface Draft {
    id: number;
    name: string;
    when: { field: string; op: Op; value: string }[];
    category: string;
    tags: string;
    certain: boolean;
    because: string;
    enabled: boolean;
    priority: number;
  }

  let rules = $state<Rule[]>([]);
  let draft = $state<Draft | null>(null);

  async function load() {
    rules = await api.rules();
  }
  onMount(load);

  function describe(r: Rule) {
    const when = r.def.when
      .map((c) => {
        const v = noValue(c.op) ? '' : ` ${Array.isArray(c.value) ? c.value.join(', ') : String(c.value)}`;
        if (c.op === 'eq' && c.value === true) return fieldLabel(c.field);
        return `${fieldLabel(c.field)} ${opLabel(c)}${v}`;
      })
      .join(' and ');
    const then = [
      r.def.then.category ? (app.categories.find((x) => x.key === r.def.then.category)?.name ?? r.def.then.category) : null,
      ...r.def.then.tags,
    ]
      .filter(Boolean)
      .join(', ');
    return `${when} → ${then}${r.def.then.category && r.def.then.certain ? ', without review' : ''}`;
  }

  function edit(r: Rule | null) {
    draft = r
      ? {
          id: r.id,
          name: r.name,
          when: r.def.when.map((c) => ({ field: c.field, op: c.op, value: Array.isArray(c.value) ? c.value.join(', ') : c.value == null ? '' : String(c.value) })),
          category: r.def.then.category ?? '',
          tags: r.def.then.tags.join(', '),
          certain: r.def.then.certain,
          because: r.def.then.because,
          enabled: r.enabled,
          priority: r.priority,
        }
      : { id: 0, name: '', when: [{ field: 'name', op: 'regex', value: '' }], category: '', tags: '', certain: false, because: '', enabled: true, priority: 5 };
  }

  function toCondition(c: Draft['when'][number]): Condition {
    const kind = fieldKind(c.field);
    let value: Condition['value'] = null;
    if (!noValue(c.op)) {
      if (c.op === 'in') value = c.value.split(',').map((s) => s.trim()).filter(Boolean);
      else if (kind === 'number') value = Number(c.value);
      else value = c.value;
    }
    return { field: c.field, op: c.op, value };
  }

  async function save() {
    if (!draft) return;
    const d = draft;
    const existing = rules.find((r) => r.id === d.id);
    const rule: Rule = {
      id: d.id,
      key: existing?.key ?? null,
      name: d.name.trim() || 'My rule',
      enabled: d.enabled,
      priority: d.priority,
      builtin: existing?.builtin ?? false,
      def: {
        when: d.when.map(toCondition),
        then: {
          category: d.category || null,
          // Rules the owner writes are trusted like their own decisions only
          // when they say so.
          confidence: d.certain ? 1 : 0.9,
          certain: d.certain,
          tags: d.tags.split(',').map((s) => s.trim()).filter(Boolean),
          because: d.because.trim() || `your rule “${d.name.trim() || 'My rule'}”`,
        },
      },
    };
    try {
      await api.saveRule(rule);
      draft = null;
      await load();
      toast.show('Saved the rule. Roll sorts the library with it now.');
    } catch (e) {
      toast.error(e);
    }
  }

  async function toggle(r: Rule, enabled: boolean) {
    try {
      await api.saveRule({ ...r, enabled });
      await load();
    } catch (e) {
      toast.error(e);
    }
  }

  async function remove(r: Rule) {
    if (!confirm(`Deletes the rule “${r.name}”. Items it sorted keep their category until rules run again.`)) return;
    try {
      await api.deleteRule(r.id);
      await load();
    } catch (e) {
      toast.error(e);
    }
  }
</script>

<ul class="rules" data-testid="rules">
  {#each rules as r (r.id)}
    <li class:off={!r.enabled}>
      <Switch label="Use {r.name}" checked={r.enabled} onchange={(v) => void toggle(r, v)} />
      <div class="text">
        <span class="name">{r.name}{#if r.builtin}<span class="muted builtin">built in</span>{/if}</span>
        <span class="desc">{describe(r)}</span>
      </div>
      {#if !r.builtin}
        <IconButton label="Edit {r.name}" onclick={() => edit(r)}><Pencil size={14} strokeWidth={1.5} /></IconButton>
        <IconButton label="Delete {r.name}" onclick={() => void remove(r)}><Trash2 size={14} strokeWidth={1.5} /></IconButton>
      {/if}
    </li>
  {/each}
</ul>

{#if draft}
  <div class="editor" role="group" aria-label={draft.id ? 'Edit rule' : 'New rule'}>
    <label class="field"><span>Name</span><Input bind:value={draft.name} placeholder="Receipts from the scanner app" aria-label="Rule name" /></label>
    <div class="field">
      <span>When all of these are true</span>
      {#each draft.when as c, i (i)}
        <div class="cond">
          <select
            bind:value={c.field}
            aria-label="Field"
            onchange={() => {
              const ops = OPS[fieldKind(c.field)];
              if (!ops.some((o) => o[0] === c.op)) c.op = ops[0]![0];
            }}
          >
            {#each FIELDS as [k, label] (k)}<option value={k}>{label}</option>{/each}
          </select>
          <select bind:value={c.op} aria-label="Condition">
            {#each OPS[fieldKind(c.field)] as [k, label] (k)}<option value={k}>{label}</option>{/each}
          </select>
          {#if !noValue(c.op)}
            <Input
              mono={c.op === 'regex'}
              bind:value={c.value}
              placeholder={c.op === 'regex' ? '^IMG_\\d+' : c.op === 'in' ? 'png, gif' : ''}
              aria-label="Value"
            />
          {/if}
          {#if draft.when.length > 1}
            <IconButton label="Remove condition" onclick={() => draft?.when.splice(i, 1)}><X size={14} strokeWidth={1.5} /></IconButton>
          {/if}
        </div>
      {/each}
      <div><Button variant="ghost" onclick={() => draft?.when.push({ field: 'name', op: 'regex', value: '' })}><Plus size={14} strokeWidth={1.5} /> Add condition</Button></div>
      <span class="hint">Patterns are regular expressions; (?i) at the start ignores case.</span>
    </div>
    <div class="then">
      <label class="field">
        <span>Category</span>
        <select bind:value={draft.category} aria-label="Category">
          <option value="">No category, tags only</option>
          {#each app.categories.filter((c) => c.key !== 'unsorted') as c (c.id)}<option value={c.key}>{c.parent_id ? `  ${c.name}` : c.name}</option>{/each}
        </select>
      </label>
      <label class="field"><span>Tags</span><Input bind:value={draft.tags} placeholder="Source/Scanner, Receipts" aria-label="Tags" /></label>
    </div>
    <label class="toggle">
      <Switch label="Decide without review" bind:checked={draft.certain} />
      <span>Decide without review <span class="muted">Off: matching items are proposed for review instead.</span></span>
    </label>
    <label class="field">
      <span>Reason shown on items</span>
      <Input bind:value={draft.because} placeholder="a receipt from the scanner app" aria-label="Reason" />
      <span class="hint">Shown as “Documents, because of …”.</span>
    </label>
    <div class="row">
      <Button variant="ghost" onclick={() => (draft = null)}>Cancel</Button>
      <Button variant="primary" onclick={save} disabled={!draft.when.every((c) => noValue(c.op) || c.value.trim()) || (!draft.category && !draft.tags.trim())}>
        Save rule
      </Button>
    </div>
  </div>
{:else}
  <Button onclick={() => edit(null)}><Plus size={16} strokeWidth={1.5} /> Add rule</Button>
{/if}

<style>
  .rules {
    margin: 0 0 16px;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 6px 8px 6px 12px;
    border-bottom: 1px solid var(--border);
  }
  li:last-child {
    border-bottom: 0;
  }
  .builtin {
    margin-left: 8px;
    font-size: var(--t12);
  }
  li.off .text {
    opacity: 0.55;
  }
  .text {
    display: grid;
    gap: 2px;
    flex: 1;
    min-width: 0;
    font-size: var(--t13);
  }
  .desc,
  .muted,
  .hint {
    color: var(--muted);
  }
  .desc,
  .hint {
    font-size: var(--t12);
  }
  .desc {
    overflow-wrap: anywhere;
  }
  .editor {
    display: grid;
    gap: 14px;
    max-width: 640px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  .field {
    display: grid;
    gap: 6px;
    font-size: var(--t13);
  }
  .cond {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .then {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  select {
    height: 32px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t13);
    flex: none;
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
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
