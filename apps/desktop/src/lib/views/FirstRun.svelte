<script lang="ts">
  import { ArrowLeft, Check, Folder } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import Button from '../ui/Button.svelte';
  import Input from '../ui/Input.svelte';
  import Choice from '../ui/Choice.svelte';

  const STEPS = ['Welcome', 'Library', 'Saving changes', 'AI', 'Immich', 'Start'] as const;
  let step = $state(0);
  let folder = $state<string | null>(app.roots[0]?.path ?? null);
  let writeMode = $state(app.settings?.write_mode ?? 'file_and_sidecar');
  let aiMode = $state(app.settings?.ai_mode ?? 'off');
  let openrouterKey = $state('');
  let immichUrl = $state(app.settings?.immich_url ?? '');
  let immichKey = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  const canContinue = $derived(step !== 1 || folder !== null);

  async function choose() {
    const picked = await api.pickFolder('Choose the library folder');
    if (picked) folder = picked;
  }

  async function finish() {
    if (!folder || !app.settings) return;
    busy = true;
    error = null;
    try {
      if (aiMode === 'openrouter' && openrouterKey.trim()) await api.setSecret('openrouter', openrouterKey);
      const immich = immichUrl.trim();
      if (immich && immichKey.trim()) await api.setSecret('immich', immichKey);
      if (!app.roots.some((r) => r.path === folder)) await api.addRoot(folder);
      await app.saveSettings({
        first_run_done: true,
        write_mode: writeMode,
        ai_mode: aiMode,
        immich_url: immich || null,
        network: {
          ...app.settings.network,
          ai: aiMode === 'openrouter',
          immich: Boolean(immich),
        },
      });
      await app.refresh();
      app.go('timeline');
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function next() {
    if (step < STEPS.length - 1) step++;
    else void finish();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && canContinue && !busy && !(e.target instanceof HTMLInputElement)) {
      e.preventDefault();
      next();
    }
  }
</script>

<svelte:window {onkeydown} />

<main class="first-run">
  <div class="card" data-testid="first-run">
    <div
      class="progress mono"
      role="progressbar"
      aria-label="Setup"
      aria-valuemin={1}
      aria-valuemax={STEPS.length}
      aria-valuenow={step + 1}
      aria-valuetext="Step {step + 1} of {STEPS.length}: {STEPS[step]}"
    >
      {#each STEPS as s, i (s)}
        <span class:done={i < step} class:current={i === step}></span>
      {/each}
    </div>

    {#if step === 0}
      <h1>Welcome to Roll</h1>
      <p>Roll sorts a library of photos and videos into folders by year, fixes dates and places in bulk, and sends the part you choose to Immich.</p>
      <ul class="points">
        <li>It works on your folders where they are. Nothing is uploaded unless you turn it on.</li>
        <li>Every change is shown before it happens, and you can undo it.</li>
        <li>Files are never deleted for good. Roll's trash goes to the Recycle Bin when you empty it.</li>
      </ul>
    {:else if step === 1}
      <h1>Choose the library folder</h1>
      <p>This folder becomes the complete archive. Roll reads everything inside it and organizes it in place. Databases and caches stay in Roll's own app folder.</p>
      <button class="folder" onclick={choose} data-testid="choose-folder">
        <Folder size={16} strokeWidth={1.5} />
        {#if folder}
          <span class="mono path">{folder}</span>
          <span class="muted">Change…</span>
        {:else}
          <span>Choose folder…</span>
        {/if}
      </button>
      <p class="hint muted">You can add more folders later in Settings.</p>
    {:else if step === 2}
      <h1>How to save changes</h1>
      <p>Dates, places, tags and ratings you fix in Roll are written as standard metadata.</p>
      <div class="choices" role="radiogroup" aria-label="How to save changes">
        <Choice
          selected={writeMode === 'file_and_sidecar'}
          onselect={() => (writeMode = 'file_and_sidecar')}
          title="In the files and as sidecars"
          badge="Recommended"
        >
          <li>Windows Explorer and the Photos app show the fixed dates, places and tags.</li>
          <li>Immich, digiKam and Lightroom read the <span class="mono">.xmp</span> sidecars.</li>
          <li>Only metadata is rewritten, never the image data.</li>
        </Choice>
        <Choice selected={writeMode === 'sidecar_only'} onselect={() => (writeMode = 'sidecar_only')} title="Sidecars only">
          <li>Originals stay untouched, byte for byte.</li>
          <li>Explorer and the Photos app don't see the changes.</li>
        </Choice>
      </div>
    {:else if step === 3}
      <h1>Choose AI</h1>
      <p>Rules and local models sort most items for free. A small vision model can decide what is left.</p>
      <div class="choices" role="radiogroup" aria-label="AI">
        <Choice selected={aiMode === 'local'} onselect={() => (aiMode = 'local')} title="Local">
          <li>Uses Ollama, LM Studio or llama.cpp on this computer. Free, and nothing leaves it.</li>
        </Choice>
        <Choice selected={aiMode === 'openrouter'} onselect={() => (aiMode = 'openrouter')} title="OpenRouter">
          <li>Small thumbnails without metadata go to a provider that keeps no data. About $0.30–2.50 per 100,000 items.</li>
          <li>You see the cost and give consent before every run.</li>
        </Choice>
        <Choice selected={aiMode === 'off'} onselect={() => (aiMode = 'off')} title="Off">
          <li>Rules and local image embeddings only.</li>
        </Choice>
      </div>
      {#if aiMode === 'openrouter'}
        <label class="field">
          <span>OpenRouter API key</span>
          <Input type="password" placeholder="sk-or-…" bind:value={openrouterKey} autocomplete="off" />
          <span class="muted hint">Stored in the system keychain. You can add it later in Settings.</span>
        </label>
      {/if}
    {:else if step === 4}
      <h1>Connect Immich</h1>
      <p>Optional. Immich gets only what you choose: by default your photos and confirmed events. Nothing is sent until you have reviewed the first sync plan.</p>
      <label class="field">
        <span>Server address</span>
        <Input placeholder="https://photos.example.com" bind:value={immichUrl} />
      </label>
      <label class="field">
        <span>API key</span>
        <Input type="password" placeholder="Needs access to assets, albums and tags" bind:value={immichKey} autocomplete="off" />
        <span class="muted hint">Stored in the system keychain.</span>
      </label>
    {:else}
      <h1>Ready</h1>
      <dl class="summary">
        <dt>Library</dt>
        <dd class="mono">{folder}</dd>
        <dt>Changes</dt>
        <dd>{writeMode === 'file_and_sidecar' ? 'In the files and as sidecars' : 'Sidecars only'}</dd>
        <dt>AI</dt>
        <dd>{aiMode === 'local' ? 'Local' : aiMode === 'openrouter' ? 'OpenRouter' : 'Off'}</dd>
        <dt>Immich</dt>
        <dd>{immichUrl.trim() || 'Not connected'}</dd>
      </dl>
      <p>Roll starts reading the library. The grid fills while indexing runs. Nothing in your folders changes until you apply a plan.</p>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    {/if}

    <footer>
      {#if step > 0}
        <Button variant="ghost" onclick={() => step--} disabled={busy}><ArrowLeft size={16} strokeWidth={1.5} /> Back</Button>
      {/if}
      <span class="spacer"></span>
      {#if step === 4}
        <Button variant="ghost" onclick={() => ((immichUrl = ''), (immichKey = ''), next())}>Skip</Button>
      {/if}
      <Button variant="primary" size="md" onclick={next} disabled={!canContinue || busy} data-testid="continue">
        {#if step === STEPS.length - 1}<Check size={16} strokeWidth={1.5} /> Start{:else}Continue{/if}
      </Button>
    </footer>
  </div>
</main>

<style>
  .first-run {
    display: grid;
    place-items: center;
    height: 100%;
    padding: 24px;
    overflow: auto;
    background: var(--surface);
  }
  .card {
    width: 560px;
    max-width: 100%;
    padding: 32px;
    border: 1px solid var(--border);
    border-radius: var(--r-sheet);
    background: var(--bg);
    animation: rise 200ms var(--ease);
  }
  .progress {
    display: flex;
    gap: 4px;
    margin-bottom: 28px;
  }
  .progress span {
    flex: 1;
    height: 2px;
    background: var(--border);
    transition: background 200ms var(--ease);
  }
  .progress .done,
  .progress .current {
    background: var(--fg);
  }
  h1 {
    margin: 0 0 12px;
    font-size: var(--t24);
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  p {
    margin: 0 0 16px;
    color: var(--muted);
  }
  .points {
    margin: 0 0 8px;
    padding-left: 18px;
    color: var(--fg);
  }
  .points li {
    margin-bottom: 6px;
  }
  .folder {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 44px;
    padding: 0 14px;
    border: 1px dashed var(--border-strong);
    border-radius: var(--r-btn);
    background: var(--surface);
    cursor: pointer;
    text-align: left;
  }
  .folder:hover {
    border-color: var(--fg);
  }
  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--t13);
  }
  .hint {
    display: block;
    margin-top: 8px;
    font-size: var(--t12);
  }
  .choices {
    display: grid;
    gap: 8px;
  }
  .field {
    display: grid;
    gap: 6px;
    margin-top: 16px;
    font-size: var(--t13);
  }
  .summary {
    display: grid;
    grid-template-columns: 96px 1fr;
    gap: 8px 12px;
    margin: 0 0 16px;
    font-size: var(--t13);
  }
  .summary dt {
    color: var(--muted);
  }
  .summary dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .error {
    color: var(--danger);
  }
  footer {
    display: flex;
    gap: 8px;
    margin-top: 28px;
  }
  .spacer {
    flex: 1;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
