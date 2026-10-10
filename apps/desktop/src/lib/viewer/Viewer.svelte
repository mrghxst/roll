<script lang="ts">
  import { ChevronLeft, ChevronRight, CircleDot, ExternalLink, FolderOpen, Info, X } from '@lucide/svelte';
  import { onMount } from 'svelte';
  import { api } from '../ipc/api';
  import type { AssetDetail } from '../ipc/bindings/AssetDetail';
  import InfoPanel from '../shell/InfoPanel.svelte';
  import { viewer } from '../state/viewer.svelte';
  import { shortDate, time } from '../util/format';
  import Filmstrip from './Filmstrip.svelte';
  import { playable } from './video';

  let stage: HTMLDivElement | undefined = $state();
  let stageW = $state(0);
  let stageH = $state(0);
  let detail = $state<AssetDetail | null>(null);
  let infoOpen = $state(false);
  let fullLoaded = $state(false);
  let previewLoaded = $state(false);
  let scale = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let opening = $state(true);
  let liveOn = $state(false);
  let videoNote = $state<string | null>(null);

  const id = $derived(viewer.current);
  const reduced = typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

  let seq = 0;
  $effect(() => {
    const target = id;
    if (target == null) return;
    const my = ++seq;
    fullLoaded = false;
    previewLoaded = false;
    resetZoom();
    liveOn = false;
    videoNote = null;
    void api.assetDetail(target).then((d) => {
      if (my === seq) detail = d;
    });
  });

  // Preload neighbors' previews.
  $effect(() => {
    if (!viewer.ids) return;
    for (const d of [1, -1, 2]) {
      const nid = viewer.ids[viewer.index + d];
      if (nid !== undefined) new Image().src = api.previewUrl(nid, 1);
    }
  });

  const isVideo = $derived(detail?.kind === 'video');
  const fit = $derived.by(() => {
    const w = detail?.width ?? 4;
    const h = detail?.height ?? 3;
    const s = Math.min(stageW / w, stageH / h, 1e9);
    return { w: w * s, h: h * s, natural: w };
  });

  // FLIP: start from the tile's rectangle and zoom to the fitted image.
  const origin = viewer.origin;
  onMount(() => {
    requestAnimationFrame(() => requestAnimationFrame(() => (opening = false)));
  });
  const flip = $derived.by(() => {
    if (!opening || !origin || !stage || reduced) return '';
    const r = stage.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height / 2;
    const s = Math.max(origin.width / fit.w, origin.height / fit.h);
    return `translate(${origin.left + origin.width / 2 - cx}px, ${origin.top + origin.height / 2 - cy}px) scale(${s})`;
  });

  function resetZoom() {
    scale = 1;
    panX = 0;
    panY = 0;
  }

  function zoomAt(factor: number, clientX: number, clientY: number) {
    if (!stage) return;
    const r = stage.getBoundingClientRect();
    const max = Math.max(4, (fit.natural / fit.w) * 2);
    const next = Math.max(1, Math.min(max, scale * factor));
    const ox = clientX - r.left - r.width / 2 - panX;
    const oy = clientY - r.top - r.height / 2 - panY;
    panX -= ox * (next / scale - 1);
    panY -= oy * (next / scale - 1);
    scale = next;
    if (scale === 1) resetZoom();
  }

  function oneToOne(clientX: number, clientY: number) {
    if (scale > 1) resetZoom();
    else zoomAt(fit.natural / fit.w, clientX, clientY);
  }

  let drag = $state<{ x: number; y: number; px: number; py: number } | null>(null);

  function close() {
    viewer.close();
  }

  function onkeydown(e: KeyboardEvent) {
    switch (e.key) {
      case 'Escape':
        e.preventDefault();
        if (scale > 1) resetZoom();
        else close();
        break;
      case 'ArrowRight':
        viewer.step(1);
        break;
      case 'ArrowLeft':
        viewer.step(-1);
        break;
      case ' ':
        if (viewer.quick) {
          e.preventDefault();
          close();
        }
        break;
      case 'i':
      case 'I':
        infoOpen = !infoOpen;
        break;
      case '+':
      case '=':
        zoomAt(1.25, innerWidth / 2, innerHeight / 2);
        break;
      case '-':
        zoomAt(0.8, innerWidth / 2, innerHeight / 2);
        break;
      default:
        return;
    }
    e.stopPropagation();
  }

  const liveMotion = $derived(detail?.group_kind === 'live' ? (detail.group_members.find((m) => m !== detail?.id) ?? null) : null);
</script>

<svelte:window onkeydown={onkeydown} />

<div class="viewer" class:quick={viewer.quick} role="dialog" aria-modal="true" aria-label={detail?.name ?? 'Viewer'}>
  <div class="frame">
    <header>
      <button class="icon" aria-label="Close" onclick={close}><X size={18} strokeWidth={1.5} /></button>
      {#if detail}
        <div class="title">
          <span>{detail.taken_local ? shortDate(detail.taken_local) : detail.name}</span>
          <span class="mono dim">{time(detail.taken_local)}</span>
        </div>
      {/if}
      <span class="spacer"></span>
      {#if liveMotion != null}
        <button
          class="icon"
          class:on={liveOn}
          aria-label="Play Live Photo"
          onpointerdown={() => (liveOn = true)}
          onpointerup={() => (liveOn = false)}
          onpointerleave={() => (liveOn = false)}><CircleDot size={18} strokeWidth={1.5} /></button
        >
      {/if}
      {#if id != null && !viewer.quick}
        <button class="icon" aria-label="Show in Explorer" onclick={() => void api.showInFolder(id)}><FolderOpen size={18} strokeWidth={1.5} /></button>
        <button class="icon" aria-label="Open with the default app" onclick={() => void api.openWithDefault(id)}><ExternalLink size={18} strokeWidth={1.5} /></button>
      {/if}
      <button class="icon" class:on={infoOpen} aria-label="Info (I)" onclick={() => (infoOpen = !infoOpen)}><Info size={18} strokeWidth={1.5} /></button>
    </header>

    <div class="row">
      <div
        class="stage"
        bind:this={stage}
        bind:clientWidth={stageW}
        bind:clientHeight={stageH}
        role="presentation"
        onwheel={(e) => {
          if (isVideo) return;
          e.preventDefault();
          zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX, e.clientY);
        }}
        ondblclick={(e) => !isVideo && oneToOne(e.clientX, e.clientY)}
        onpointerdown={(e) => {
          if (scale > 1) {
            drag = { x: e.clientX, y: e.clientY, px: panX, py: panY };
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
          }
        }}
        onpointermove={(e) => {
          if (drag) {
            panX = drag.px + e.clientX - drag.x;
            panY = drag.py + e.clientY - drag.y;
          }
        }}
        onpointerup={() => (drag = null)}
      >
        {#if id != null && detail && detail.id === id}
          {#if isVideo}
            {@const native = playable(detail.format, detail.codec)}
            <!-- svelte-ignore a11y_media_has_caption -->
            <video
              class="media"
              style:width="{fit.w}px"
              style:height="{fit.h}px"
              src={api.videoUrl(id, native)}
              poster={api.previewUrl(id, detail.thumb_v)}
              controls
              autoplay={!viewer.quick}
              onloadstart={() => (videoNote = native ? null : 'Preparing this video for playback…')}
              oncanplay={() => (videoNote = null)}
              onerror={() => (videoNote = 'This video could not be played. Open it with the default app instead.')}
            ></video>
          {:else}
            <div
              class="media"
              class:grab={scale > 1}
              style:width="{fit.w}px"
              style:height="{fit.h}px"
              style:transform={flip || `translate(${panX}px, ${panY}px) scale(${scale})`}
              style:transition={opening || drag ? 'none' : undefined}
            >
              <img src={api.thumbUrl(id, detail.thumb_v)} alt="" class="layer low" draggable="false" />
              <img
                src={api.previewUrl(id, detail.thumb_v)}
                alt=""
                class="layer"
                class:show={previewLoaded}
                draggable="false"
                onload={() => (previewLoaded = true)}
              />
              <img
                src={api.fullUrl(id, detail.thumb_v)}
                alt={detail.description ?? detail.name}
                class="layer"
                class:show={fullLoaded}
                draggable="false"
                decoding="async"
                onload={() => (fullLoaded = true)}
              />
              {#if liveOn && liveMotion != null}
                <!-- svelte-ignore a11y_media_has_caption -->
                <video class="layer show" src={api.videoUrl(liveMotion, true)} autoplay playsinline></video>
              {/if}
            </div>
          {/if}
        {/if}
        {#if videoNote}<p class="note">{videoNote}</p>{/if}
        {#if viewer.ids && viewer.index > 0}
          <button class="nav prev" aria-label="Previous" onclick={() => viewer.step(-1)}><ChevronLeft size={24} strokeWidth={1.5} /></button>
        {/if}
        {#if viewer.ids && viewer.index < viewer.ids.length - 1}
          <button class="nav next" aria-label="Next" onclick={() => viewer.step(1)}><ChevronRight size={24} strokeWidth={1.5} /></button>
        {/if}
      </div>
      {#if infoOpen}<InfoPanel id={id} dark />{/if}
    </div>
    {#if !viewer.quick && viewer.ids}
      <Filmstrip ids={viewer.ids} index={viewer.index} onpick={(i) => (viewer.index = i)} />
    {/if}
  </div>
</div>

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 50;
    background: #000;
    color: #fff;
    animation: fade 180ms var(--ease);
  }
  .quick {
    background: rgba(0, 0, 0, 0.72);
    padding: 4vh 6vw;
  }
  .frame {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .quick .frame {
    border-radius: var(--r-sheet);
    overflow: hidden;
    background: #000;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  }
  header {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 48px;
    padding: 0 8px;
    flex: none;
  }
  .title {
    display: flex;
    gap: 8px;
    margin-left: 8px;
    font-size: var(--t14);
  }
  .dim {
    color: #a1a1a1;
  }
  .spacer {
    flex: 1;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: #d4d4d4;
    cursor: pointer;
  }
  .icon:hover,
  .icon.on {
    background: #1f1f1f;
    color: #fff;
  }
  .row {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .stage {
    position: relative;
    flex: 1;
    display: grid;
    place-items: center;
    overflow: hidden;
    touch-action: none;
  }
  .media {
    position: relative;
    transition: transform 200ms var(--ease);
    will-change: transform;
  }
  .grab {
    cursor: grab;
  }
  .layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    opacity: 0;
    transition: opacity 160ms var(--ease);
  }
  .layer.low,
  .layer.show {
    opacity: 1;
  }
  video.media {
    background: #000;
  }
  .nav {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    width: 44px;
    height: 64px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    color: #d4d4d4;
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms var(--ease);
  }
  .stage:hover .nav {
    opacity: 1;
  }
  .nav:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .prev {
    left: 8px;
  }
  .next {
    right: 8px;
  }
  .note {
    position: absolute;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    margin: 0;
    padding: 6px 12px;
    border-radius: var(--r-btn);
    background: rgba(0, 0, 0, 0.7);
    font-size: var(--t13);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
