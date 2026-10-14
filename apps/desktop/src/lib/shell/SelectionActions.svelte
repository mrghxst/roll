<script lang="ts">
  import { ArchiveRestore, CalendarClock, Cloud, ExternalLink, FolderInput, FolderOpen, GraduationCap, Heart, MapPin, RotateCcw, RotateCw, ScanSearch, Star, Tag, Trash2 } from '@lucide/svelte';
  import { api } from '../ipc/api';
  import { app } from '../state/app.svelte';
  import { rate, restore, rotate, setCategory, setImmich, toggleFavorite, trash } from '../state/edits';
  import { selection } from '../state/selection.svelte';
  import { toast } from '../state/toast.svelte';
  import IconButton from '../ui/IconButton.svelte';
  import Menu, { type MenuItem } from '../ui/Menu.svelte';

  let { onemptytrash }: { onemptytrash: (ids: number[]) => void } = $props();

  const ids = $derived(selection.list());
  const single = $derived(ids.length === 1 ? ids[0]! : null);
  const inTrash = $derived(app.view === 'trash');

  const categoryItems = $derived.by<MenuItem[]>(() => {
    const top = app.categories.filter((c) => c.parent_id === null && c.key !== 'unsorted');
    const out: MenuItem[] = [];
    for (const c of top) {
      out.push({ label: c.name, run: () => void setCategory(ids, c.key) });
      for (const s of app.categories.filter((x) => x.parent_id === c.id)) {
        out.push({ label: s.name, depth: 1, run: () => void setCategory(ids, s.key) });
      }
    }
    out.push({ label: 'Unsorted', run: () => void setCategory(ids, null) });
    return out;
  });

  const immichItems = $derived<MenuItem[]>([
    { label: 'Send to Immich', run: () => void setImmich(ids, 'send') },
    { label: 'Keep local only', run: () => void setImmich(ids, 'local') },
    { label: 'Use the default (category and event)', run: () => void setImmich(ids, 'default') },
  ]);

  const seeing = $derived(app.pack('image-model')?.state === 'installed');

  /** Show the items that look most like the selection. */
  async function findSimilar() {
    try {
      const like = await api.similarItems(ids, 300);
      app.go('timeline', { key: 'similar', label: `Look like the ${ids.length === 1 ? 'selected item' : `${ids.length} selected`}`, query: { ids: like } });
    } catch (e) {
      toast.error(e);
    }
  }

  const ratingItems = $derived<MenuItem[]>(
    [5, 4, 3, 2, 1, 0].map((r) => ({ label: r ? '★'.repeat(r) : 'No rating', hint: String(r), run: () => void rate(ids, r) })),
  );
</script>

{#if inTrash}
  <IconButton label="Restore" onclick={() => void restore(ids).then(() => selection.clear())} data-testid="restore">
    <ArchiveRestore size={16} strokeWidth={1.5} />
  </IconButton>
  <IconButton label="Send to the Recycle Bin…" onclick={() => onemptytrash(ids)} data-testid="recycle">
    <Trash2 size={16} strokeWidth={1.5} />
  </IconButton>
{:else}
  <Menu label="Set category" items={categoryItems}>
    {#snippet trigger({ open, toggle })}
      <IconButton label="Set category" active={open} onclick={toggle} data-testid="set-category"><FolderInput size={16} strokeWidth={1.5} /></IconButton>
    {/snippet}
  </Menu>
  <IconButton label="Add a tag (T)" onclick={() => (app.tagging = ids)} data-testid="add-tag"><Tag size={16} strokeWidth={1.5} /></IconButton>
  {#if seeing}
    <IconButton label="Find similar items" onclick={() => void findSimilar()} data-testid="find-similar"><ScanSearch size={16} strokeWidth={1.5} /></IconButton>
  {/if}
  <IconButton label="Teach a category with these examples" onclick={() => (app.teaching = ids)} data-testid="teach"><GraduationCap size={16} strokeWidth={1.5} /></IconButton>
  <IconButton label="Set location (L)" onclick={() => (app.locating = ids)} data-testid="set-location"><MapPin size={16} strokeWidth={1.5} /></IconButton>
  <IconButton label="Change date (D)" onclick={() => (app.dating = ids)} data-testid="change-date"><CalendarClock size={16} strokeWidth={1.5} /></IconButton>
  <IconButton label="Favorite (F)" onclick={() => void toggleFavorite(ids)} data-testid="favorite"><Heart size={16} strokeWidth={1.5} /></IconButton>
  <Menu label="Rating" items={ratingItems}>
    {#snippet trigger({ open, toggle })}
      <IconButton label="Rating" active={open} onclick={toggle} data-testid="rating"><Star size={16} strokeWidth={1.5} /></IconButton>
    {/snippet}
  </Menu>
  <Menu label="Immich" items={immichItems}>
    {#snippet trigger({ open, toggle })}
      <IconButton label="Immich" active={open} onclick={toggle} data-testid="immich-menu"><Cloud size={16} strokeWidth={1.5} /></IconButton>
    {/snippet}
  </Menu>
  <IconButton label="Rotate left" onclick={() => void rotate(ids, -1)}><RotateCcw size={16} strokeWidth={1.5} /></IconButton>
  <IconButton label="Rotate right" onclick={() => void rotate(ids, 1)}><RotateCw size={16} strokeWidth={1.5} /></IconButton>
  <IconButton
    label="Move to the trash (Del)"
    onclick={() => {
      void trash(ids);
      selection.clear();
    }}
    data-testid="trash"><Trash2 size={16} strokeWidth={1.5} /></IconButton
  >
  {#if single != null}
    <span class="sep"></span>
    <IconButton label="Show in Explorer" onclick={() => void api.showInFolder(single)}><FolderOpen size={16} strokeWidth={1.5} /></IconButton>
    <IconButton label="Open with the default app" onclick={() => void api.openWithDefault(single)}><ExternalLink size={16} strokeWidth={1.5} /></IconButton>
  {/if}
{/if}

<style>
  .sep {
    width: 1px;
    height: 20px;
    margin: 0 4px;
    background: var(--border);
  }
</style>
