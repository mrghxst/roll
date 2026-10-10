import { SvelteSet } from 'svelte/reactivity';

/** The ids selected in the current grid, shared with the top bar. */
class SelectionState {
  ids = new SvelteSet<number>();
  /** Item index that Shift-click extends from. */
  anchor = $state(-1);

  get size() {
    return this.ids.size;
  }

  has(id: number) {
    return this.ids.has(id);
  }

  set(ids: Iterable<number>) {
    this.ids.clear();
    for (const id of ids) this.ids.add(id);
  }

  add(ids: Iterable<number>) {
    for (const id of ids) this.ids.add(id);
  }

  toggle(id: number) {
    if (this.ids.has(id)) this.ids.delete(id);
    else this.ids.add(id);
  }

  /** Select all of `ids`, or clear them if all are selected already. */
  toggleAll(ids: number[]) {
    if (ids.every((id) => this.ids.has(id))) for (const id of ids) this.ids.delete(id);
    else this.add(ids);
  }

  clear() {
    this.ids.clear();
    this.anchor = -1;
  }

  list() {
    return [...this.ids];
  }
}

export const selection = new SelectionState();
