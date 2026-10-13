<script lang="ts">
  import { FolderPlus, RefreshCw, Trash2 } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { api } from '../ipc/api';
  import type { Settings } from '../ipc/bindings/Settings';
  import { app } from '../state/app.svelte';
  import Button from '../ui/Button.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Input from '../ui/Input.svelte';
  import Kbd from '../ui/Kbd.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Switch from '../ui/Switch.svelte';
  import { n } from '../util/format';
  import { SHORTCUTS } from '../util/shortcuts';
  import Categories from '../settings/Categories.svelte';
  import Rules from '../settings/Rules.svelte';
  import Packs from '../settings/Packs.svelte';

  const SECTIONS = [
    ['library', 'Library'],
    ['categories', 'Categories'],
    ['rules', 'Rules'],
    ['saving', 'Saving changes'],
    ['appearance', 'Appearance'],
    ['ai', 'AI'],
    ['immich', 'Immich'],
    ['packs', 'Models and data'],
    ['network', 'Network'],
    ['performance', 'Performance'],
    ['shortcuts', 'Shortcuts'],
    ['advanced', 'Advanced'],
    ['about', 'About'],
  ] as const;
  let section = $state<(typeof SECTIONS)[number][0]>('library');
  let hasKey = $state({ openrouter: false, immich: false });
  let keyDraft = $state({ openrouter: '', immich: '' });
  let message = $state<string | null>(null);

  onMount(async () => {
    hasKey = { openrouter: await api.hasSecret('openrouter'), immich: await api.hasSecret('immich') };
  });

  const s = $derived(app.settings!);

  function save(patch: Partial<Settings>) {
    void app.saveSettings(patch);
  }

  async function addFolder() {
    const p = await api.pickFolder('Add a library folder');
    if (!p) return;
    try {
      await api.addRoot(p);
      await app.refresh();
    } catch (e) {
      message = String(e);
    }
  }

  async function saveKey(name: 'openrouter' | 'immich') {
    await api.setSecret(name, keyDraft[name]);
    keyDraft[name] = '';
    hasKey[name] = await api.hasSecret(name);
  }

  async function removeKey(name: 'openrouter' | 'immich') {
    await api.deleteSecret(name);
    hasKey[name] = false;
  }

  const NETWORK: [keyof Settings['network'], string, string][] = [
    ['immich', 'Your Immich server', 'Uploads and updates the items you send to Immich.'],
    ['ai', 'AI provider', 'Sends 512 px thumbnails without metadata to OpenRouter, only after you agree to each run.'],
    ['downloads', 'Downloads of models, maps and place names', 'Fetches packs listed in Settings, checked against their SHA-256.'],
    ['online_tiles', 'Online map tiles', 'Detailed maps from OpenFreeMap. The server sees which map areas you look at.'],
    ['online_search', 'Online place search', 'Searches place names online when the offline list has no match.'],
    ['update_checks', 'Update checks', 'Asks GitHub whether a newer version of Roll exists.'],
  ];
</script>

<div class="settings">
  <nav aria-label="Settings sections">
    {#each SECTIONS as [k, label] (k)}
      <button class:active={section === k} onclick={() => (section = k)}>{label}</button>
    {/each}
  </nav>
  <div class="page">
    {#if message}<p class="message" role="status">{message}</p>{/if}

    {#if section === 'library'}
      <h1>Library</h1>
      <p class="lead">Folders that make up the archive. Roll indexes them and organizes inside them.</p>
      <ul class="list">
        {#each app.roots as r (r.id)}
          <li>
            <span class="mono path">{r.path}</span>
            <span class="muted mono">{n(r.count)} items</span>
            <IconButton
              label="Remove from Roll"
              onclick={async () => {
                if (confirm(`Removes ${r.path} from Roll. The files stay where they are.`)) {
                  await api.removeRoot(r.id);
                  await app.refresh();
                }
              }}><Trash2 size={16} strokeWidth={1.5} /></IconButton
            >
          </li>
        {/each}
      </ul>
      <div class="row">
        <Button onclick={addFolder}><FolderPlus size={16} strokeWidth={1.5} /> Add folder…</Button>
        <Button variant="ghost" onclick={() => void api.rescan()}><RefreshCw size={16} strokeWidth={1.5} /> Check for changes</Button>
      </div>
    {:else if section === 'categories'}
      <h1>Categories</h1>
      <p class="lead">What Roll sorts items into, and the folder each category gets when you organize. Subcategories are tags unless they get their own folder.</p>
      <Categories />
    {:else if section === 'rules'}
      <h1>Rules</h1>
      <p class="lead">
        Rules sort items by file name, camera, size and other facts, before any model looks at them. Your rules run first; the first rule that sets a
        category wins, and tags from every matching rule apply.
      </p>
      <Rules />
    {:else if section === 'saving'}
      <h1>Saving changes</h1>
      <p class="lead">How fixed dates, places, tags and ratings are written.</p>
      <Segmented
        label="Write mode"
        options={[
          { value: 'file_and_sidecar', label: 'In the files and as sidecars' },
          { value: 'sidecar_only', label: 'Sidecars only' },
        ]}
        value={s.write_mode}
        onchange={(v) => save({ write_mode: v })}
      />
      <p class="note">
        {#if s.write_mode === 'file_and_sidecar'}
          Explorer and the Photos app show the changes. Only metadata is rewritten, never the image data, and every write can be undone.
        {:else}
          Originals stay untouched byte for byte. Explorer and the Photos app don't see the changes; Immich, digiKam and Lightroom read the sidecars.
        {/if}
      </p>
    {:else if section === 'appearance'}
      <h1>Appearance</h1>
      <div class="field">
        <span>Theme</span>
        <Segmented
          label="Theme"
          options={[
            { value: 'system', label: 'System' },
            { value: 'light', label: 'Light' },
            { value: 'dark', label: 'Dark' },
          ]}
          value={s.theme}
          onchange={(v) => save({ theme: v })}
        />
      </div>
      <div class="field">
        <span>Grid density</span>
        <Segmented
          label="Grid density"
          options={[
            { value: '140', label: 'Small' },
            { value: '200', label: 'Medium' },
            { value: '300', label: 'Large' },
          ]}
          value={String(s.row_height <= 170 ? 140 : s.row_height >= 250 ? 300 : 200)}
          onchange={(v) => save({ row_height: Number(v) })}
        />
      </div>
    {:else if section === 'ai'}
      <h1>AI</h1>
      <p class="lead">Rules and local models decide first. A decision model only sees what is still uncertain.</p>
      <Segmented
        label="AI mode"
        options={[
          { value: 'local', label: 'Local' },
          { value: 'openrouter', label: 'OpenRouter' },
          { value: 'off', label: 'Off' },
        ]}
        value={s.ai_mode}
        onchange={(v) => save({ ai_mode: v, network: { ...s.network, ai: v === 'openrouter' ? s.network.ai : false } })}
      />
      {#if s.ai_mode === 'local'}
        <label class="field">
          <span>Local server</span>
          <Input placeholder="Detected automatically (Ollama, LM Studio, llama.cpp)" value={s.ai_local_endpoint ?? ''} onchange={(e) => save({ ai_local_endpoint: e.currentTarget.value.trim() || null })} />
        </label>
      {:else if s.ai_mode === 'openrouter'}
        <label class="field">
          <span>OpenRouter API key {#if hasKey.openrouter}<span class="muted">· saved in the system keychain</span>{/if}</span>
          <div class="row">
            <Input type="password" placeholder={hasKey.openrouter ? 'Replace the saved key' : 'sk-or-…'} bind:value={keyDraft.openrouter} autocomplete="off" />
            <Button onclick={() => saveKey('openrouter')} disabled={!keyDraft.openrouter.trim()}>Save</Button>
            {#if hasKey.openrouter}<Button variant="ghost" onclick={() => removeKey('openrouter')}>Remove</Button>{/if}
          </div>
        </label>
        <label class="field">
          <span>Budget per run</span>
          <Input type="number" min="0.1" step="0.5" mono value={s.ai_budget_usd} onchange={(e) => save({ ai_budget_usd: Number(e.currentTarget.value) || 5 })} />
        </label>
      {/if}
      {#if s.ai_mode !== 'off'}
        <div class="field">
          <span>Items per request</span>
          <Segmented
            label="Items per request"
            options={[
              { value: '8', label: '8' },
              { value: '4', label: '4' },
              { value: '1', label: '1' },
            ]}
            value={String(s.ai_batch)}
            onchange={(v) => save({ ai_batch: Number(v) })}
          />
          <span class="muted">Fewer items per request costs more. Use 4 or 1 only when the model check shows a model does worse with 8.</span>
        </div>
        <div class="row">
          <Button variant="ghost" onclick={() => app.go('ai')}>Open Sort with AI</Button>
        </div>
      {/if}
      <div class="toggle">
        <Switch label="Allow Private items in cloud models" checked={s.ai_allow_private_cloud} onchange={(v) => save({ ai_allow_private_cloud: v })} />
        <div><strong>Allow Private items in cloud models</strong><p class="muted">Off: items tagged Private never leave this computer.</p></div>
      </div>
      <div class="toggle">
        <Switch label="Allow providers that keep data" checked={s.ai_allow_non_zdr} onchange={(v) => save({ ai_allow_non_zdr: v })} />
        <div><strong>Allow providers that keep data</strong><p class="muted">Off: only zero-data-retention endpoints are used.</p></div>
      </div>
    {:else if section === 'immich'}
      <h1>Immich</h1>
      <p class="lead">Immich gets the part of the archive you choose. Every sync shows its plan first.</p>
      <label class="field">
        <span>Server address</span>
        <Input placeholder="https://photos.example.com" value={s.immich_url ?? ''} onchange={(e) => save({ immich_url: e.currentTarget.value.trim() || null })} />
      </label>
      <label class="field">
        <span>API key {#if hasKey.immich}<span class="muted">· saved in the system keychain</span>{/if}</span>
        <div class="row">
          <Input type="password" placeholder={hasKey.immich ? 'Replace the saved key' : 'Needs access to assets, albums and tags'} bind:value={keyDraft.immich} autocomplete="off" />
          <Button onclick={() => saveKey('immich')} disabled={!keyDraft.immich.trim()}>Save</Button>
          {#if hasKey.immich}<Button variant="ghost" onclick={() => removeKey('immich')}>Remove</Button>{/if}
        </div>
      </label>
    {:else if section === 'packs'}
      <h1>Models and data</h1>
      <p class="lead">
        Place names and the models Roll runs on this computer. Nothing downloads until you choose to; each download shows its size, source and license
        first.
      </p>
      <Packs />
    {:else if section === 'network'}
      <h1>Network</h1>
      <p class="lead">Everything that can reach the internet. All of it is off unless you turned it on.</p>
      {#each NETWORK as [k, title, desc] (k)}
        <div class="toggle">
          <Switch label={title} checked={s.network[k]} onchange={(v) => save({ network: { ...s.network, [k]: v } })} />
          <div><strong>{title}</strong><p class="muted">{desc}</p></div>
        </div>
      {/each}
      <p class="note">Roll sends no telemetry. Logs stay on this computer.</p>
    {:else if section === 'performance'}
      <h1>Performance</h1>
      <label class="field">
        <span>Background threads</span>
        <Input type="number" min="1" max="64" mono placeholder="Automatic" value={s.threads ?? ''} onchange={(e) => save({ threads: Number(e.currentTarget.value) || null })} />
        <span class="muted hint">Takes effect after a restart.</span>
      </label>
      <label class="field">
        <span>Preview cache</span>
        <Input type="number" min="0.5" step="0.5" mono value={s.preview_cache_gb} onchange={(e) => save({ preview_cache_gb: Number(e.currentTarget.value) || 5 })} />
        <span class="muted hint">GB of 1024 px previews kept for the viewer.</span>
      </label>
      <div class="toggle">
        <Switch label="Use the GPU" checked={s.use_gpu} onchange={(v) => save({ use_gpu: v })} />
        <div><strong>Use the GPU</strong><p class="muted">For image embeddings. Turn off if other apps need it.</p></div>
      </div>
      <div class="toggle">
        <Switch label="Only when idle" checked={s.only_when_idle} onchange={(v) => save({ only_when_idle: v })} />
        <div><strong>Only when idle</strong><p class="muted">Pause heavy work while you use the computer.</p></div>
      </div>
      <div class="toggle">
        <Switch label="Pause on battery" checked={s.pause_on_battery} onchange={(v) => save({ pause_on_battery: v })} />
        <div><strong>Pause on battery</strong></div>
      </div>
    {:else if section === 'shortcuts'}
      <h1>Shortcuts</h1>
      <dl class="keys">
        {#each SHORTCUTS as [keys, what] (keys + what)}
          <dt><Kbd {keys} /></dt>
          <dd>{what}</dd>
        {/each}
      </dl>
    {:else if section === 'advanced'}
      <h1>Advanced</h1>
      <div class="row">
        <Button onclick={() => void api.rescan()}><RefreshCw size={16} strokeWidth={1.5} /> Check all folders for changes</Button>
        <Button
          variant="ghost"
          onclick={async () => {
            await api.clearLogs();
            message = 'Logs cleared.';
          }}>Clear logs</Button
        >
      </div>
      <p class="note mono">App data: {app.info?.data_dir}</p>
    {:else if section === 'about'}
      <h1>About Roll</h1>
      <p class="mono">Version {app.info?.version}</p>
      <dl class="about">
        <dt>ExifTool</dt>
        <dd class="mono">{app.info?.exiftool ?? 'Not found. Metadata changes cannot be saved.'}</dd>
        <dt>FFmpeg</dt>
        <dd class="mono">{app.info?.ffmpeg ?? 'Not found. HEIC and videos cannot be shown.'}</dd>
      </dl>
      <p class="note">
        Roll is MIT licensed. It uses ExifTool by Phil Harvey (Perl Artistic License), FFmpeg (LGPL), SQLite (public domain), the Geist fonts (OFL),
        Lucide icons (ISC), and map data from OpenStreetMap contributors (ODbL).
      </p>
      {#if app.packs.some((p) => p.state === 'installed')}
        <p class="note">
          {#each app.packs.filter((p) => p.state === 'installed') as p (p.id)}{p.attribution}.<br />{/each}
        </p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .settings {
    display: flex;
    height: 100%;
  }
  nav {
    width: 200px;
    flex: none;
    padding: 16px 8px;
    border-right: 1px solid var(--border);
  }
  nav button {
    display: block;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-btn);
    background: none;
    color: var(--muted);
    text-align: left;
    font-size: var(--t13);
    cursor: pointer;
  }
  nav button:hover {
    color: var(--fg);
  }
  nav button.active {
    color: var(--fg);
    background: var(--surface);
  }
  .page {
    flex: 1;
    overflow-y: auto;
    padding: 24px 40px 48px;
  }
  .page > :global(*) {
    max-width: 640px;
  }
  h1 {
    margin: 0 0 8px;
    font-size: var(--t24);
    font-weight: 600;
  }
  .lead {
    margin: 0 0 20px;
    color: var(--muted);
  }
  .list {
    margin: 0 0 16px;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
  }
  .list li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    padding: 0 8px 0 12px;
    border-bottom: 1px solid var(--border);
    font-size: var(--t13);
  }
  .list li:last-child {
    border-bottom: 0;
  }
  .path {
    flex: 1;
    overflow-wrap: anywhere;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .field {
    display: grid;
    gap: 6px;
    margin: 16px 0;
    font-size: var(--t13);
  }
  .hint {
    font-size: var(--t12);
  }
  .toggle {
    display: flex;
    gap: 12px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border);
    font-size: var(--t13);
  }
  .toggle strong {
    font-weight: 500;
  }
  .toggle p {
    margin: 2px 0 0;
  }
  .note {
    margin-top: 16px;
    color: var(--muted);
    font-size: var(--t13);
  }
  .message {
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    font-size: var(--t13);
  }
  .keys,
  .about {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 10px 20px;
    font-size: var(--t13);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  dt {
    color: var(--muted);
  }
</style>
