<script lang="ts">
  import { Plus, X } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import type { AssetDetail } from '../ipc/bindings/AssetDetail';
  import { app } from '../state/app.svelte';
  import { removeTag } from '../state/edits';
  import { selection } from '../state/selection.svelte';
  import { accuracy, bytes, coords, duration, n, offset, shortDate, time } from '../util/format';

  let { id, dark = false }: { id: number | null; dark?: boolean } = $props();

  let detail = $state<AssetDetail | null>(null);
  let seq = 0;
  $effect(() => {
    const target = id;
    void app.revision;
    const my = ++seq;
    if (target == null) {
      detail = null;
      return;
    }
    void api.assetDetail(target).then((d) => {
      if (my === seq) detail = d;
    });
  });

  const why = (field: string) => detail?.why.filter((w) => w.field === field) ?? [];
  const sources = $derived(detail?.tags.filter((t) => t.startsWith('Source/')).map((t) => t.slice(7)) ?? []);
  const otherTags = $derived(detail?.tags.filter((t) => !t.startsWith('Source/') && !t.startsWith('Category/')) ?? []);
  const camera = $derived(detail ? [detail.make && !detail.model?.startsWith(detail.make) ? detail.make : null, detail.model].filter(Boolean).join(' ') : '');
</script>

<aside class="info" class:dark aria-label="Info">
  {#if detail}
    <section>
      <h3>Date</h3>
      {#if detail.taken_local}
        <p class="big">{shortDate(detail.taken_local)}</p>
        <p class="mono">{time(detail.taken_local)} {offset(detail.tz_offset)}</p>
      {:else}
        <p class="muted">No date</p>
      {/if}
      {#each why('date') as w (w.text)}<p class="why">{w.text}</p>{/each}
      <div class="links"><button onclick={() => detail && (app.dating = [detail.id])}>Change date…</button></div>
    </section>

    <section>
      <h3>Place</h3>
      {#if detail.lat != null && detail.lon != null}
        {#if detail.place}<p>{detail.place}</p>{/if}
        <p class="mono">{coords(detail.lat, detail.lon)}</p>
        {#if detail.loc_acc}<p class="muted mono">{accuracy(detail.loc_acc)}</p>{/if}
      {:else}
        <p class="muted">No location</p>
      {/if}
      {#each why('location') as w (w.text)}<p class="why">{w.text}</p>{/each}
      <div class="links">
        <button onclick={() => detail && (app.locating = [detail.id])}>{detail.lat != null ? 'Change' : 'Set location…'}</button>
        {#if detail.lat != null && detail.lon != null}
          <button
            onclick={() => {
              if (detail?.lat != null && detail.lon != null)
                app.copiedPlace = { lat: detail.lat, lon: detail.lon, accuracy_m: detail.loc_acc ?? 10, label: detail.place ?? coords(detail.lat, detail.lon) };
            }}>Copy</button
          >
        {/if}
      </div>
    </section>

    {#if camera || detail.lens}
      <section>
        <h3>Camera</h3>
        {#if camera}<p>{camera}</p>{/if}
        {#if detail.lens}<p class="muted">{detail.lens}</p>{/if}
      </section>
    {/if}

    <section>
      <h3>Category</h3>
      <p>
        {detail.category_name ?? 'Unsorted'}
        {#if detail.category_conf != null && detail.review < 3}<span class="muted mono"> {Math.round(detail.category_conf * 100)}%</span>{/if}
      </p>
      {#each why('category') as w (w.text)}<p class="why">{w.text}</p>{/each}
      {#if detail.rating > 0}
        <p class="rating" aria-label="Rated {detail.rating} of 5">{'★'.repeat(detail.rating)}<span class="muted">{'★'.repeat(5 - detail.rating)}</span></p>
      {/if}
    </section>

    <section>
      <h3>Tags</h3>
      <div class="tags">
        {#each otherTags as t (t)}
          <span class="tag">
            {t}
            <button aria-label="Remove tag {t}" onclick={() => detail && void removeTag([detail.id], t)}><X size={11} strokeWidth={2} /></button>
          </span>
        {/each}
        <button class="add" onclick={() => detail && (app.tagging = [detail.id])}><Plus size={12} strokeWidth={1.5} /> Add tag</button>
      </div>
      {#if sources.length}<p class="muted">Source: {sources.join(', ')}</p>{/if}
    </section>

    {#if detail.description}
      <section>
        <h3>Description</h3>
        <p class="desc">{detail.description}</p>
      </section>
    {/if}

    <section>
      <h3>File</h3>
      <p class="name">{detail.name}</p>
      <p class="muted mono">
        {detail.format.toUpperCase()} · {bytes(detail.size)}{#if detail.width && detail.height} · {detail.width}×{detail.height}{/if}{#if detail.duration_ms} · {duration(detail.duration_ms)}{/if}
      </p>
      {#if detail.codec}<p class="muted mono">{detail.codec}{detail.fps ? ` · ${Math.round(detail.fps)} fps` : ''}{detail.has_audio === false ? ' · no sound' : ''}</p>{/if}
      <p class="path mono" title={detail.path}>{detail.rel_path}</p>
      {#if detail.group_kind}
        <p class="muted">
          {detail.group_kind === 'live' ? 'Live Photo' : detail.group_kind === 'raw' ? 'RAW + JPEG' : detail.group_kind === 'edit' ? 'Edited, with original' : 'Burst'}
          · {n(detail.group_members.length)} files
        </p>
      {/if}
      {#if detail.sidecars.length}<p class="muted">Sidecars: {detail.sidecars.map((s) => s.split('/').pop()).join(', ')}</p>{/if}
    </section>
  {:else if selection.size > 1}
    <p class="empty">{n(selection.size)} items selected.</p>
  {:else}
    <p class="empty">Select an item to see its details.</p>
  {/if}
</aside>

<style>
  .info {
    width: var(--info);
    flex: none;
    height: 100%;
    overflow-y: auto;
    padding: 8px 20px 24px;
    border-left: 1px solid var(--border);
    background: var(--bg);
    font-size: var(--t13);
    user-select: text;
  }
  .dark {
    --bg: #0a0a0a;
    --fg: #fff;
    --muted: #a1a1a1;
    --border: #262626;
    color: var(--fg);
  }
  section {
    padding: 14px 0;
    border-bottom: 1px solid var(--border);
  }
  section:last-child {
    border-bottom: 0;
  }
  h3 {
    margin: 0 0 6px;
    color: var(--muted);
    font-size: var(--t12);
    font-weight: 500;
  }
  p {
    margin: 0 0 2px;
  }
  .big {
    font-size: var(--t16);
    font-weight: 500;
  }
  .why {
    margin-top: 6px;
    color: var(--muted);
    font-size: var(--t12);
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 4px;
  }
  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 2px 2px 6px;
    border: 1px solid var(--border);
    border-radius: var(--r-1);
    font-size: var(--t12);
  }
  .tag button,
  .add {
    display: inline-flex;
    align-items: center;
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .tag button {
    width: 16px;
    height: 16px;
    justify-content: center;
    padding: 0;
    border-radius: 3px;
  }
  .tag button:hover,
  .add:hover {
    color: var(--fg);
  }
  .add {
    gap: 3px;
    height: 22px;
    padding: 0 4px;
    font-size: var(--t12);
  }
  .links {
    display: flex;
    gap: 10px;
    margin-top: 4px;
  }
  .links button {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    font-size: var(--t12);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .links button:hover {
    color: var(--fg);
  }
  .rating {
    margin-top: 4px;
    letter-spacing: 2px;
  }
  .name {
    overflow-wrap: anywhere;
    font-weight: 500;
  }
  .path {
    margin-top: 4px;
    color: var(--muted);
    font-size: 11px;
    overflow-wrap: anywhere;
  }
  .desc {
    white-space: pre-wrap;
  }
  .empty {
    margin-top: 24px;
    color: var(--muted);
  }
</style>
