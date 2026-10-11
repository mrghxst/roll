<script lang="ts">
  import { api } from '../ipc/api';
  import { n } from '../util/format';

  type Props = { ids: number[]; total: number };
  let { ids, total }: Props = $props();

  let picked = $state(new Set<number>());

  $effect(() => {
    void ids;
    picked = new Set();
  });

  function click(e: MouseEvent, id: number) {
    const next = new Set(e.ctrlKey || e.metaKey ? picked : []);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    picked = next;
  }

  function dragstart(e: DragEvent, id: number) {
    const list = picked.has(id) ? [...picked] : [id];
    e.dataTransfer?.setData('application/x-roll-ids', JSON.stringify(list));
    e.dataTransfer?.setData('text/plain', `${list.length} items`);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
  }
</script>

<section class="tray" aria-label="Items without a location">
  <header>
    <span>No location in this range <span class="mono muted">({n(total)})</span></span>
    <span class="hint">{picked.size ? `${n(picked.size)} picked · ` : ''}Drag onto the map to place them. Ctrl+click picks several.</span>
    <button class="all" onclick={() => (picked = new Set(ids))} disabled={!ids.length}>Pick all shown</button>
  </header>
  <div class="strip" data-testid="tray">
    {#each ids as id (id)}
      <button class="thumb" class:picked={picked.has(id)} draggable="true" ondragstart={(e) => dragstart(e, id)} onclick={(e) => click(e, id)} aria-pressed={picked.has(id)} aria-label="Item {id}">
        <img src={api.thumbUrl(id, 1)} alt="" loading="lazy" draggable="false" />
      </button>
    {:else}
      <p class="empty">Every item in this range has a location.</p>
    {/each}
  </div>
</section>

<style>
  .tray {
    display: flex;
    flex-direction: column;
    height: 124px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    font-size: var(--t13);
  }
  .muted,
  .hint {
    color: var(--muted);
  }
  .hint {
    flex: 1;
    font-size: var(--t12);
  }
  .all {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    color: var(--fg);
    font-size: var(--t12);
    cursor: pointer;
  }
  .strip {
    display: flex;
    gap: 4px;
    padding: 0 12px 8px;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .thumb {
    flex: none;
    width: 72px;
    height: 72px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: var(--r-1);
    background: var(--surface);
    cursor: grab;
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
  .empty {
    margin: 16px 0;
    color: var(--muted);
    font-size: var(--t13);
  }
</style>
