<script lang="ts">
  import { api } from '../ipc/api';

  let { ids, index, onpick }: { ids: Uint32Array; index: number; onpick: (i: number) => void } = $props();

  const RADIUS = 20;
  const window_ = $derived.by(() => {
    const out: number[] = [];
    for (let i = Math.max(0, index - RADIUS); i <= Math.min(ids.length - 1, index + RADIUS); i++) out.push(i);
    return out;
  });
</script>

<div class="strip" aria-label="Filmstrip">
  <div class="track">
    {#each window_ as i (i)}
      <button
        class="cell"
        class:current={i === index}
        style:left="{(i - index) * 50}px"
        aria-label="Item {i + 1}"
        aria-current={i === index}
        onclick={() => onpick(i)}
      >
        <img src={api.thumbUrl(ids[i] ?? 0, 0)} alt="" draggable="false" loading="lazy" />
      </button>
    {/each}
  </div>
</div>

<style>
  .strip {
    position: relative;
    height: 64px;
    flex: none;
    overflow: hidden;
  }
  .track {
    position: absolute;
    left: 50%;
    top: 8px;
  }
  .cell {
    position: absolute;
    top: 0;
    width: 48px;
    height: 48px;
    margin-left: -24px;
    padding: 0;
    border: 0;
    background: #111;
    opacity: 0.55;
    cursor: pointer;
    transition:
      left 200ms var(--ease),
      opacity 120ms var(--ease);
  }
  .cell:hover {
    opacity: 0.85;
  }
  .current {
    opacity: 1;
    box-shadow: 0 0 0 2px #fff;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
</style>
