<script lang="ts">
  import { api } from '../ipc/api';
  import type { FaceBox } from '../ipc/bindings/FaceBox';

  let { face, size = 96, round = true }: { face: FaceBox; size?: number; round?: boolean } = $props();

  // A square around the face, 1.6 times its larger side, from the preview.
  const crop = $derived.by(() => {
    const ratio = face.ratio > 0 ? face.ratio : 1;
    const side = Math.min(1, 1.6 * Math.max(face.w, face.h / ratio));
    const cx = face.x + face.w / 2;
    const cy = face.y + face.h / 2;
    const width = 100 / side;
    return { width, left: 50 - cx * width, top: 50 - (cy * width) / ratio };
  });
</script>

<span class="face" class:round style:width="{size}px" style:height="{size}px">
  <img
    src={api.previewUrl(face.asset, 1)}
    alt=""
    loading="lazy"
    style:width="{crop.width}%"
    style:left="{crop.left}%"
    style:top="{crop.top}%"
  />
</span>

<style>
  .face {
    position: relative;
    display: inline-block;
    overflow: hidden;
    flex: none;
    border-radius: var(--r-1);
    background: var(--surface);
  }
  .face.round {
    border-radius: 50%;
  }
  img {
    position: absolute;
    max-width: none;
    height: auto;
  }
</style>
