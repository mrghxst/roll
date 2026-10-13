<script lang="ts">
  import { Play, RefreshCw, Settings as Gear, Sparkles } from '@lucide/svelte';
  import { untrack } from 'svelte';
  import { api } from '../ipc/api';
  import type { AiStatus } from '../ipc/bindings/AiStatus';
  import type { CheckItem } from '../ipc/bindings/CheckItem';
  import type { CheckReport } from '../ipc/bindings/CheckReport';
  import type { DecidePlan } from '../ipc/bindings/DecidePlan';
  import type { ModelOffer } from '../ipc/bindings/ModelOffer';
  import type { RunInfo } from '../ipc/bindings/RunInfo';
  import { app } from '../state/app.svelte';
  import { toast } from '../state/toast.svelte';
  import { viewer } from '../state/viewer.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Kbd from '../ui/Kbd.svelte';
  import Switch from '../ui/Switch.svelte';
  import { n, plural } from '../util/format';

  let status = $state<AiStatus | null>(null);
  let offers = $state<ModelOffer[]>([]);
  let offersError = $state<string | null>(null);
  let plan = $state<DecidePlan | null>(null);
  let planError = $state<string | null>(null);
  let runs = $state<RunInfo[]>([]);
  let report = $state<CheckReport | null>(null);
  let filter = $state('');
  let showAll = $state(false);
  let budget = $state<number | null>(null);
  let checkModels = $state<string[]>([]);
  let labeling = $state(false);
  let items = $state<CheckItem[]>([]);
  let at = $state(0);

  const s = $derived(app.settings);
  const cloud = $derived(s?.ai_mode === 'openrouter');
  const running = $derived(app.activeJobs.find((j) => j.kind === 'decide'));
  const usd = (x: number) => (x > 0 && x < 0.01 ? `$${x.toFixed(4)}` : `$${x.toFixed(2)}`);
  const pct = (x: number) => `${Math.round(x * 100)}%`;

  async function loadStatus() {
    try {
      status = await api.aiStatus();
    } catch (e) {
      toast.error(e);
    }
  }

  async function loadOffers() {
    offersError = null;
    try {
      offers = await api.aiOffers();
    } catch (e) {
      offersError = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadRest() {
    try {
      [runs, report] = await Promise.all([api.aiRuns(), api.aiCheckReport()]);
    } catch (e) {
      toast.error(e);
    }
    try {
      plan = await api.aiPlan();
      planError = null;
      untrack(() => (budget ??= plan!.budget_usd));
    } catch (e) {
      plan = null;
      planError = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    void s?.ai_mode;
    void s?.network.ai;
    void s?.ai_local_endpoint;
    untrack(() => void loadStatus());
  });
  $effect(() => {
    if (cloud && s?.network.ai) untrack(() => void loadOffers());
  });
  $effect(() => {
    void app.aiRevision;
    void app.revision;
    void s?.ai_decision_model;
    void s?.ai_mode;
    untrack(() => void loadRest());
  });

  const zdrOnly = $derived(!(s?.ai_allow_non_zdr && showAll));
  const shown = $derived(
    offers.filter((o) => (!zdrOnly || o.zdr) && (!filter.trim() || `${o.id} ${o.name} ${o.provider ?? ''}`.toLowerCase().includes(filter.trim().toLowerCase()))),
  );
  const localModels = $derived(status?.local.find((l) => l.base === status?.endpoint)?.models ?? status?.local[0]?.models ?? []);
  const bestLocal = $derived(localModels.find((m) => m.best));
  const choices = $derived(cloud ? offers.filter((o) => o.zdr || s?.ai_allow_non_zdr).map((o) => o.id) : localModels.filter((m) => m.installed).map((m) => m.name));

  async function copyPull(name: string) {
    try {
      await navigator.clipboard.writeText(`ollama pull ${name}`);
      toast.show(`Copied "ollama pull ${name}". Run it in a terminal, then look again.`);
    } catch {
      toast.show(`Run "ollama pull ${name}" in a terminal, then look again.`);
    }
  }

  async function pick(model: string) {
    try {
      await api.aiPick(model, offers.find((o) => o.id === model) ?? null);
      toast.show(`Decisions now use ${model}`);
    } catch (e) {
      toast.error(e);
    }
  }

  async function start() {
    if (!plan) return;
    try {
      await api.aiStart(cloud ? budget : null);
      toast.show('The run started. Proposals appear in Review as they come in.');
    } catch (e) {
      toast.error(e);
    }
  }

  async function resume(r: RunInfo) {
    try {
      await api.aiResume(r.id, r.state === 'capped' && budget != null && budget > (r.budget_usd ?? 0) ? budget : null);
    } catch (e) {
      toast.error(e);
    }
  }

  // The model check: label a sample, then compare up to 3 models on it.

  async function sample() {
    try {
      const count = await api.aiCheckSample();
      toast.show(`Picked ${plural(count, 'item')} across your categories. Items you already reviewed count as labelled.`);
      await openLabels();
    } catch (e) {
      toast.error(e);
    }
  }

  async function openLabels() {
    items = await api.aiCheckItems();
    at = Math.max(0, items.findIndex((i) => !i.truth));
    labeling = true;
  }

  const keyed = $derived(app.categories.filter((c) => c.parent_id === null && c.key !== 'unsorted').slice(0, 9));
  const cur = $derived(items[at]);
  const labeledCount = $derived(items.filter((i) => i.truth).length);

  async function label(key: string | null) {
    const it = cur;
    if (!it) return;
    try {
      await api.aiCheckLabel(it.id, key);
      it.truth = key;
      report = await api.aiCheckReport();
      const next = items.findIndex((i, j) => j > at && !i.truth);
      at = next >= 0 ? next : Math.min(at + 1, items.length - 1);
    } catch (e) {
      toast.error(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (!labeling || app.view !== 'ai' || viewer.ids || app.paletteOpen || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT') return;
    if (/^[1-9]$/.test(e.key)) {
      const c = keyed[Number(e.key) - 1];
      if (c) void label(c.key);
    } else if (e.key === 'Enter' && cur?.proposed) {
      e.preventDefault();
      void label(cur.proposed);
    } else if (e.key === 'ArrowRight' || e.key === 's') at = Math.min(at + 1, items.length - 1);
    else if (e.key === 'ArrowLeft' || e.key === 'Backspace') at = Math.max(at - 1, 0);
    else if (e.key === 'Escape') labeling = false;
  }

  function toggleCheck(m: string) {
    checkModels = checkModels.includes(m) ? checkModels.filter((x) => x !== m) : checkModels.length < 3 ? [...checkModels, m] : checkModels;
  }

  async function runCheck() {
    try {
      await api.aiCheckStart(checkModels);
      labeling = false;
      toast.show(`Checking ${plural(checkModels.length, 'model')} on ${plural(report?.labeled ?? labeledCount, 'labelled item')}.`);
    } catch (e) {
      toast.error(e);
    }
  }

  const name = (k: string | null) => app.categories.find((c) => c.key === k)?.name ?? k ?? '';
  const STATES: Record<string, string> = {
    ready: 'Waiting',
    running: 'Running',
    stopped: 'Stopped',
    capped: 'Stopped at the budget',
    done: 'Done',
    failed: 'Failed',
  };
</script>

<svelte:window {onkeydown} />

<div class="ai">
  <header>
    <h1>Sort with AI</h1>
    <p class="lead">
      Rules and local models decide first. A decision model looks at the items that are still uncertain and proposes a category for each; the
      proposals wait in Review, the most doubtful first.
    </p>
  </header>

  <section class="card" data-testid="ai-model">
    <h2>Decision model</h2>
    {#if !status}
      <p class="muted">Checking…</p>
    {:else if status.mode === 'off'}
      <p>AI is off. Rules and your own decisions still sort everything they can.</p>
      <div class="row"><Button onclick={() => app.go('settings')}><Gear size={14} strokeWidth={1.5} /> Choose Local or OpenRouter</Button></div>
    {:else if status.mode === 'local'}
      {#if !status.local.length && !status.endpoint}
        <p>No local AI server answers on its usual port (Ollama 11434, LM Studio 1234, llama.cpp 8080). Start one, or enter its address in Settings › AI.</p>
        <div class="row"><Button onclick={() => void loadStatus()}><RefreshCw size={14} strokeWidth={1.5} /> Look again</Button></div>
      {:else}
        <p>
          {status.local.map((l) => l.name).join(', ') || 'Local server'} at <span class="mono">{status.endpoint}</span>. Nothing leaves this computer.
          {#if status.gpu_memory_gb}Your graphics card has {status.gpu_memory_gb} GB.{/if}
        </p>
        {#if bestLocal}
          <p class="best" data-testid="best-local">
            Highest quality{status.gpu_memory_gb ? ' that fits your graphics card' : ''}: <strong class="mono">{bestLocal.name}</strong>
            ({bestLocal.quality}{bestLocal.measured != null ? `, ${pct(bestLocal.measured)} on your items` : ''}{bestLocal.download_gb ? `, ${bestLocal.download_gb} GB` : ''}){bestLocal.installed ? '' : ', not downloaded yet'}.
          </p>
        {/if}
        <table class="offers" data-testid="local-models">
          <thead><tr><th>Quality</th><th>Model</th><th class="num">Download</th><th class="num">Memory</th><th></th></tr></thead>
          <tbody>
            {#each localModels as m (m.name)}
              <tr class:picked={status.model === m.name}>
                <td title={m.basis ?? 'No published figure Roll knows of. The model check measures it on your items.'}>
                  {m.quality}{#if m.measured != null}<span class="muted"> · {pct(m.measured)} on your items</span>{/if}
                </td>
                <td><span class="mono">{m.name}</span>{#if m.basis}<div class="muted">{m.basis}</div>{/if}</td>
                <td class="num mono">{m.download_gb != null ? `${m.download_gb} GB` : '—'}</td>
                <td class="num mono">{m.memory_gb != null ? `about ${m.memory_gb} GB` : '—'}{#if m.fits === false}<div class="muted">more than your card</div>{/if}</td>
                <td class="num">
                  {#if !m.installed}
                    <button class="copy mono" title="Copy the command" onclick={() => void copyPull(m.name)}>ollama pull {m.name}</button>
                  {:else if status.model === m.name}<span class="muted">In use</span>
                  {:else}<Button variant="ghost" onclick={() => void pick(m.name)}>Use</Button>{/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted">Quality tiers come from the models' published benchmarks; the model check below measures them on your own items. Larger models are slower.</p>
      {/if}
    {:else}
      {#if !status.has_key || !status.network_on}
        <p>{status.problem}</p>
        <div class="row"><Button onclick={() => app.go('settings')}><Gear size={14} strokeWidth={1.5} /> Open Settings</Button></div>
      {:else if offersError}
        <p class="warn">{offersError}</p>
        <div class="row"><Button onclick={() => void loadOffers()}><RefreshCw size={14} strokeWidth={1.5} /> Try again</Button></div>
      {:else}
        {#if shown[0]}
          <p class="best" data-testid="best-cloud">
            Highest quality: <strong>{shown[0].name}</strong> ({shown[0].quality}{shown[0].measured != null ? `, ${pct(shown[0].measured)} on your items` : ''}),
            {usd(shown[0].per_100k_usd)} per 100,000 items, {shown[0].zdr ? 'keeps no data' : 'may keep data'}.
          </p>
        {/if}
        <p class="muted">
          Image models on OpenRouter, highest quality first, with today's prices. The price per 100,000 items assumes every item goes to the model; usually
          30–50% do.
        </p>
        <div class="row">
          <Input placeholder="Filter models" bind:value={filter} />
          {#if s?.ai_allow_non_zdr}<Switch label="Show providers that keep data" bind:checked={showAll} /><span>Show providers that keep data</span>{/if}
        </div>
        <table class="offers" data-testid="ai-offers">
          <thead>
            <tr><th>Quality</th><th>Model</th><th class="num">≈ per 100k</th><th>Your data</th><th></th></tr>
          </thead>
          <tbody>
            {#each shown.slice(0, 40) as o (o.id)}
              <tr class:picked={status.model === o.id}>
                <td title={o.basis ?? 'No published figure Roll knows of. The model check measures it on your items.'}>
                  {o.quality}{#if o.measured != null}<span class="muted"> · {pct(o.measured)} on your items</span>{/if}
                </td>
                <td>
                  <strong>{o.name}</strong> <span class="mono muted">{o.id}</span>
                  {#if !o.structured}<span class="badge">no JSON schema</span>{/if}
                  {#if o.basis}<div class="muted">{o.basis}</div>{/if}
                </td>
                <td class="num mono">{usd(o.per_100k_usd)}<div class="muted">${o.prompt_per_m.toFixed(3)} / ${o.completion_per_m.toFixed(3)} per M</div></td>
                <td>
                  {#if o.zdr}<span class="badge">keeps no data</span>{:else}<span class="badge warn">may keep data</span>{/if}
                  <div class="muted">
                    {o.provider ?? 'Any provider'}{o.country ? `, ${o.country}` : ''}
                    {#if o.policy_url}· <button class="link" onclick={() => void api.openLink(o.policy_url!)}>privacy policy</button>{/if}
                  </div>
                </td>
                <td class="num">
                  {#if status.model === o.id}<span class="muted">In use</span>{:else}<Button variant="ghost" onclick={() => void pick(o.id)}>Use</Button>{/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted">
          "Keeps no data" means zero data retention: the provider deletes the thumbnails after answering. Roll asks OpenRouter to skip providers that collect or
          train on data either way.
        </p>
      {/if}
    {/if}
  </section>

  {#if status && status.mode !== 'off'}
    <section class="card" data-testid="ai-check">
      <h2>Model check</h2>
      <p class="muted">
        Label about 150 of your own items (around 10 minutes with the keyboard), then compare up to three models on them: accuracy, cost per 100,000 items
        and speed. The check sends only the labelled items{cloud ? ' and usually costs under $0.10' : ''}.
      </p>
      {#if report}
        <p>
          {#if report.sample}{n(report.labeled)} of {n(report.sample)} sample items labelled.{:else}No sample yet.{/if}
          {#if report.labeled}Rules decided {pct(report.local_coverage)} of them, {pct(report.local_accuracy)} right.{/if}
        </p>
      {/if}
      <div class="row">
        {#if !report?.sample}
          <Button onclick={() => void sample()} data-testid="check-sample">Pick a sample</Button>
        {:else}
          <Button onclick={() => void openLabels()} data-testid="check-label">Label items</Button>
          <Button variant="ghost" onclick={() => void sample()}>New sample</Button>
        {/if}
      </div>

      {#if labeling && cur}
        <div class="labeler" data-testid="labeler">
          <img src={api.thumbUrl(cur.id, 1)} alt="Sample item {at + 1} of {items.length}" />
          <div class="labels">
            <p class="mono muted">{at + 1} of {items.length} · {labeledCount} labelled</p>
            {#if cur.proposed}<p>Roll proposes <strong>{name(cur.proposed)}</strong>. <Kbd keys="Enter" /> agrees.</p>{/if}
            <div class="keys" role="group" aria-label="Category">
              {#each keyed as c, i (c.key)}
                <button class="key" aria-pressed={cur.truth === c.key} onclick={() => void label(c.key)}><Kbd keys={String(i + 1)} /> {c.name}</button>
              {/each}
            </div>
            <p class="muted"><Kbd keys="←" /> <Kbd keys="→" /> move · <Kbd keys="Esc" /> closes</p>
          </div>
        </div>
      {/if}

      {#if report && report.labeled >= 20}
        <div class="row">
          <span>Compare:</span>
          <div class="chips" role="group" aria-label="Models to compare">
            {#each choices.slice(0, cloud ? 8 : 6) as m (m)}
              <button class="chip" aria-pressed={checkModels.includes(m)} onclick={() => toggleCheck(m)}>{m}</button>
            {/each}
          </div>
          <Button disabled={!checkModels.length || !!running} onclick={() => void runCheck()} data-testid="check-run">Check {plural(checkModels.length, 'model')}</Button>
        </div>
      {/if}

      {#if report?.models.length}
        <table class="offers" data-testid="check-report">
          <thead><tr><th>Model</th><th class="num">Accuracy</th><th class="num">Unsure</th><th class="num">≈ per 100k</th><th class="num">Items/s</th><th></th></tr></thead>
          <tbody>
            {#each report.models as m (m.run)}
              <tr class:picked={m.recommended}>
                <td><span class="mono">{m.model}</span> {#if m.batch !== 8}<span class="badge">{m.batch} per request</span>{/if}{#if m.state === 'running'}<span class="muted"> · running</span>{/if}</td>
                <td class="num mono">{pct(m.accuracy)}</td>
                <td class="num mono">{n(m.unsure)}</td>
                <td class="num mono">{usd(m.per_100k_usd)}</td>
                <td class="num mono">{m.items_per_sec.toFixed(1)}</td>
                <td class="num">
                  {#if m.recommended}<span class="badge">Recommended</span>{/if}
                  {#if status.model !== m.model}<Button variant="ghost" onclick={() => void pick(m.model)}>Use</Button>{/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted">Recommended: the cheapest model within 2 points of the most accurate.</p>
      {/if}
    </section>

    <section class="card" data-testid="ai-run">
      <h2>Sort the uncertain items</h2>
      {#if planError}
        <p>{planError}</p>
      {:else if plan}
        <ul class="facts">
          <li><strong>{n(plan.items)}</strong> uncertain {plan.items === 1 ? 'item' : 'items'} for <span class="mono">{plan.model}</span></li>
          {#if plan.already_done}<li class="muted">{plural(plan.already_done, 'item')} already answered by this model</li>{/if}
          {#if plan.private_skipped}<li class="muted">{plural(plan.private_skipped, 'Private item')} stay on this computer</li>{/if}
          {#if plan.cloud}
            <li>Estimated cost <strong class="mono">{usd(plan.estimate_usd)}</strong> ({usd(plan.per_100k_usd)} per 100,000){plan.measured ? ', from measured token counts' : ''}</li>
          {/if}
        </ul>
        {#if plan.samples.length}
          <div class="samples">{#each plan.samples as id (id)}<img src={api.thumbUrl(id, 1)} alt="" loading="lazy" />{/each}</div>
        {/if}
        {#if plan.cloud}
          <label class="field">
            <span>Budget for this run</span>
            <Input type="number" min="0.01" step="0.5" mono value={budget ?? ''} onchange={(e) => (budget = Number(e.currentTarget.value) || null)} />
            <span class="muted">The run stops before it would pass this.</span>
          </label>
        {/if}
        <p class="consent" data-testid="ai-consent">{plan.consent}</p>
        <div class="row end">
          <Button variant="primary" disabled={!plan.items || !!running || (plan.cloud && !budget)} onclick={() => void start()} data-testid="ai-start">
            <Play size={14} strokeWidth={1.5} />
            {plan.cloud ? `Send ${n(plan.items)} thumbnails` : `Sort ${n(plan.items)} items`}
          </Button>
        </div>
      {/if}
      {#if running}
        <p class="mono" data-testid="ai-running">
          {n(running.done)} of {n(running.total)} · {usd(running.cost_usd)} spent. Stop it in Activity; it continues where it stopped.
        </p>
      {/if}
    </section>

    {#if runs.length}
      <section>
        <h2>Runs</h2>
        <ul class="runs" data-testid="ai-runs">
          {#each runs as r (r.id)}
            <li>
              <Sparkles size={14} strokeWidth={1.5} />
              <div class="name">
                <strong>{r.kind === 'check' ? 'Model check' : 'Decision run'} · <span class="mono">{r.model}</span></strong>
                <span class="muted">
                  {STATES[r.state] ?? r.state} · {n(r.done)} of {n(r.items)} · {n(r.proposed)} proposed, {n(r.unsure)} unsure{r.failed ? `, ${n(r.failed)} failed` : ''}
                  {#if r.provider === 'openrouter'} · {usd(r.spent_usd)} of {usd(r.estimate_usd)} estimated{r.budget_usd != null ? `, budget ${usd(r.budget_usd)}` : ''}{/if}
                </span>
                {#if r.note}<span class="warn">{r.note}</span>{/if}
              </div>
              {#if (r.state === 'stopped' || r.state === 'capped' || r.state === 'failed') && !running}
                <Button variant="ghost" onclick={() => void resume(r)}>Continue{r.state === 'capped' ? ' with the budget above' : ''}</Button>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/if}
</div>

<style>
  .ai {
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  .ai > * {
    max-width: 920px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  h2 {
    margin: 20px 0 8px;
    font-size: var(--t14);
    font-weight: 600;
  }
  .card h2 {
    margin-top: 0;
  }
  .lead {
    margin: 0 0 16px;
    color: var(--muted);
    max-width: 72ch;
  }
  .card {
    display: grid;
    gap: 10px;
    margin-bottom: 12px;
    padding: 14px 16px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    font-size: var(--t13);
  }
  .card p {
    margin: 0;
  }
  .muted {
    color: var(--muted);
    font-size: var(--t12);
  }
  .warn {
    color: var(--danger, var(--fg));
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .row.end {
    justify-content: flex-end;
  }
  .chips {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg);
    color: var(--muted);
    font-size: var(--t12);
    font-family: var(--mono);
    cursor: pointer;
  }
  .chip[aria-pressed='true'] {
    background: var(--fg);
    color: var(--bg);
    border-color: var(--fg);
  }
  .offers {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--t12);
  }
  .offers th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    padding: 4px 6px;
    border-bottom: 1px solid var(--border);
  }
  .offers th.num {
    text-align: right;
  }
  .offers td {
    padding: 6px;
    border-bottom: 1px solid var(--border);
    vertical-align: middle;
  }
  .offers tr.picked td {
    background: var(--surface);
  }
  .num {
    text-align: right;
    white-space: nowrap;
  }
  .badge {
    display: inline-block;
    margin-left: 4px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: var(--t11, 11px);
    color: var(--muted);
  }
  .best {
    padding: 8px 10px;
    border-left: 2px solid var(--fg);
    background: var(--surface);
  }
  .copy,
  .link {
    padding: 2px 6px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t12);
    cursor: pointer;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    text-decoration: underline;
    color: var(--muted);
  }
  .badge.warn {
    color: var(--fg);
    border-color: var(--fg);
  }
  .facts {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
  }
  .samples {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .samples img {
    width: 56px;
    height: 56px;
    object-fit: cover;
    border-radius: var(--r-1);
    background: var(--surface);
  }
  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .consent {
    padding: 10px 12px;
    border-radius: var(--r-btn);
    background: var(--surface);
  }
  .labeler {
    display: grid;
    grid-template-columns: minmax(200px, 320px) 1fr;
    gap: 16px;
    padding: 12px;
    border-radius: var(--r-btn);
    background: var(--surface);
  }
  .labeler img {
    width: 100%;
    aspect-ratio: 1;
    object-fit: contain;
    background: var(--bg);
    border-radius: var(--r-1);
  }
  .labels {
    display: grid;
    gap: 8px;
    align-content: start;
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 4px;
  }
  .key {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t12);
    cursor: pointer;
    text-align: left;
  }
  .key[aria-pressed='true'] {
    border-color: var(--fg);
    box-shadow: inset 0 0 0 1px var(--fg);
  }
  .runs {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 8px;
  }
  .runs li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .name {
    display: grid;
    flex: 1;
  }
  .name strong {
    font-weight: 500;
  }
  @media (max-width: 700px) {
    .labeler {
      grid-template-columns: 1fr;
    }
  }
</style>
