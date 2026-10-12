<script lang="ts">
  import { AlertTriangle, Check, CircleDot, Heart, Layers, Lock, Play } from '@lucide/svelte';
  import { Flag } from '../ipc/timeline';
  import { duration as fmtDuration } from '../util/format';

  let {
    url,
    x,
    y,
    w,
    h,
    color,
    flags,
    hash,
    durationMs,
    selected,
    focused,
    hidden,
    index,
    label,
  }: {
    url: string;
    x: number;
    y: number;
    w: number;
    h: number;
    color: string;
    flags: number;
    hash: string | null;
    durationMs: number | null;
    selected: boolean;
    focused: boolean;
    hidden: boolean;
    index: number;
    label: string;
  } = $props();

  let loaded = $state(false);
  let shown = $state('');

  // A recycled slot gets a new image: hide the old one until the new loads.
  $effect.pre(() => {
    if (url !== shown) loaded = false;
  });
</script>

<div
  class="tile"
  class:selected
  class:focused
  class:hidden
  data-index={index}
  id={hidden ? undefined : `tile-${index}`}
  role={hidden ? undefined : 'option'}
  aria-selected={hidden ? undefined : selected}
  aria-hidden={hidden ? true : undefined}
  aria-label={label}
  style:transform="translate({x}px, {y}px)"
  style:width="{w}px"
  style:height="{h}px"
  style:background-color={color}
  style:background-image={hash ? `url(${hash})` : undefined}
>
  {#if !hidden}
    <img
      src={url}
      alt=""
      draggable="false"
      decoding="async"
      class:loaded
      onload={() => {
        shown = url;
        loaded = true;
      }}
    />
    <span class="check" aria-hidden="true"><Check size={12} strokeWidth={2.5} /></span>
    <span class="badges">
      {#if flags & Flag.PROBLEM}<span class="badge" title="Metadata problem"><AlertTriangle size={12} strokeWidth={2} /></span>{/if}
      {#if flags & Flag.PRIVATE}<span class="badge" title="Private"><Lock size={12} strokeWidth={2} /></span>{/if}
      {#if flags & Flag.BURST}<span class="badge" title="Burst"><Layers size={12} strokeWidth={2} /></span>{/if}
      {#if flags & Flag.RAW}<span class="badge text">RAW</span>{/if}
      {#if flags & Flag.LIVE}<span class="badge" title="Live Photo"><CircleDot size={12} strokeWidth={2} /></span>{/if}
      {#if flags & Flag.VIDEO}<span class="badge text"><Play size={10} strokeWidth={2.5} fill="currentColor" />{fmtDuration(durationMs)}</span>{/if}
    </span>
    {#if flags & Flag.FAVORITE}<span class="fav" title="Favorite"><Heart size={14} strokeWidth={2} fill="currentColor" /></span>{/if}
  {/if}
</div>

<style>
  .tile {
    position: absolute;
    left: 0;
    top: 0;
    overflow: hidden;
    background-size: cover;
    contain: strict;
    cursor: default;
  }
  .hidden {
    display: none;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    opacity: 0;
    transition: opacity 160ms var(--ease);
  }
  img.loaded {
    opacity: 1;
  }
  .selected::after,
  .focused::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .selected::after {
    box-shadow: inset 0 0 0 2px var(--fg);
  }
  .focused:not(.selected)::after {
    box-shadow: inset 0 0 0 2px var(--border-strong);
  }
  .check {
    position: absolute;
    top: 6px;
    left: 6px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 1.5px solid rgba(255, 255, 255, 0.9);
    background: rgba(0, 0, 0, 0.18);
    color: transparent;
    opacity: 0;
    transition: opacity 120ms var(--ease);
  }
  .tile:hover .check {
    opacity: 1;
  }
  .selected .check {
    opacity: 1;
    border-color: var(--fg);
    background: var(--fg);
    color: var(--bg);
  }
  .badges {
    position: absolute;
    right: 6px;
    bottom: 6px;
    display: flex;
    gap: 4px;
    pointer-events: none;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 18px;
    min-width: 18px;
    justify-content: center;
    padding: 0 4px;
    border-radius: var(--r-1);
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
    font-family: var(--mono);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .fav {
    position: absolute;
    left: 6px;
    bottom: 6px;
    color: #fff;
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5));
  }
</style>
