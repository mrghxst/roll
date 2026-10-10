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

  step(d: number) {
    if (!this.ids) return;
    this.index = Math.max(0, Math.min(this.ids.length - 1, this.index + d));
  }
}

export const viewer = new ViewerState();
