// Edits on a selection, shared by the top bar, the viewer, the palette and
// the keyboard. Each one becomes an undoable batch and confirms with a toast.

import { api } from '../ipc/api';
import type { EditAction } from '../ipc/bindings/EditAction';
import { app } from './app.svelte';
import { toast } from './toast.svelte';

const plural = (n: number) => (n === 1 ? '1 item' : `${n.toLocaleString('en-US')} items`);

async function run(ids: number[], action: EditAction, done: (n: number) => string, none?: string) {
  if (!ids.length) return 0;
  try {
    const n = await api.edit(ids, action);
    if (n === 0) {
      if (none) toast.show(none);
      return 0;
    }
    toast.show(done(n), { action: { label: 'Undo', run: () => void undoLast() } });
    return n;
  } catch (e) {
    toast.error(e);
    return 0;
  }
}

export function setCategory(ids: number[], key: string | null) {
  const name = app.categories.find((c) => c.key === key)?.name ?? 'Unsorted';
  return run(ids, { kind: 'category', key }, (n) => (key ? `Moved ${plural(n)} to ${name}` : `Marked ${plural(n)} as unsorted`), `Already in ${name}`);
}

export function addTag(ids: number[], path: string) {
  return run(ids, { kind: 'add_tag', path }, (n) => `Tagged ${plural(n)} ${path}`, `Already tagged ${path}`);
}

export function removeTag(ids: number[], path: string) {
  return run(ids, { kind: 'remove_tag', path }, (n) => `Removed ${path} from ${plural(n)}`);
}

export function rate(ids: number[], rating: number) {
  return run(ids, { kind: 'rating', rating }, (n) => (rating ? `Rated ${plural(n)} ${rating} of 5` : `Cleared the rating of ${plural(n)}`));
}

/** "send", "local" or "default" (the category and event decide). */
export function setImmich(ids: number[], mode: 'send' | 'local' | 'default') {
  const done = { send: 'will go to Immich', local: 'stay on this computer only', default: 'follow the Immich defaults' }[mode];
  return run(ids, { kind: 'immich', mode }, (n) => `${plural(n)} ${done}`);
}

export function favorite(ids: number[], on: boolean) {
  return run(ids, { kind: 'favorite', on }, (n) => (on ? `Added ${plural(n)} to favorites` : `Removed ${plural(n)} from favorites`));
}

/** Favorite the items; when all of them already are, unfavorite them. */
export async function toggleFavorite(ids: number[]) {
  if (!ids.length) return;
  try {
    const n = await api.edit(ids, { kind: 'favorite', on: true });
    if (n > 0) toast.show(`Added ${plural(n)} to favorites`, { action: { label: 'Undo', run: () => void undoLast() } });
    else await favorite(ids, false);
  } catch (e) {
    toast.error(e);
  }
}

export function rotate(ids: number[], turns: 1 | -1) {
  return run(
    ids,
    { kind: 'rotate', turns },
    (n) => `Rotated ${plural(n)} ${turns < 0 ? 'left' : 'right'}`,
    'Only JPEG, PNG, WebP and TIFF photos can be rotated here.',
  );
}

export async function trash(ids: number[]) {
  if (!ids.length) return;
  try {
    await api.setTrashed(ids, true);
    toast.show(`Moved ${plural(ids.length)} to the trash`, { action: { label: 'Undo', run: () => void undoLast() } });
  } catch (e) {
    toast.error(e);
  }
}

export async function restore(ids: number[]) {
  if (!ids.length) return;
  try {
    await api.setTrashed(ids, false);
    toast.show(`Restored ${plural(ids.length)}`, { action: { label: 'Undo', run: () => void undoLast() } });
  } catch (e) {
    toast.error(e);
  }
}

export async function undoLast() {
  try {
    const label = await api.undoLast();
    // File changes are undone as a job, so this may still be running.
    toast.show(label ? `Undoing “${label}”` : 'Nothing to undo');
  } catch (e) {
    toast.error(e);
  }
}
