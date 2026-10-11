/** The open viewer: the grid's ids, the current index, and where it opened from. */
class ViewerState {
  ids = $state<Uint32Array | null>(null);
  index = $state(0);
  origin = $state<DOMRect | null>(null);
  quick = $state(false);

  open(ids: Uint32Array, index: number, origin: DOMRect | null, quick = false) {
    this.ids = ids;
    this.index = index;
    this.origin = origin;
    this.quick = quick;
  }

  close() {
    this.ids = null;
  }

  get current(): number | null {
    return this.ids ? (this.ids[this.index] ?? null) : null;
  }

  /** Drop the current item (it went to the trash); close when none are left. */
  removeCurrent() {
    if (!this.ids) return;
    const next = new Uint32Array(this.ids.length - 1);
    next.set(this.ids.subarray(0, this.index));
    next.set(this.ids.subarray(this.index + 1), this.index);
    if (!next.length) {
      this.close();
      return;
    }
    this.ids = next;
    this.index = Math.min(this.index, next.length - 1);
  }

  step(d: number) {
    if (!this.ids) return;
    this.index = Math.max(0, Math.min(this.ids.length - 1, this.index + d));
  }
}

export const viewer = new ViewerState();
