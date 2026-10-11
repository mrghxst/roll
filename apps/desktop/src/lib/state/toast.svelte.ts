/** Short messages at the bottom of the window, optionally with an action. */
export interface ToastMsg {
  id: number;
  text: string;
  action?: { label: string; run: () => void };
  error?: boolean;
}

class ToastState {
  items = $state<ToastMsg[]>([]);
  #seq = 0;

  show(text: string, opts: { action?: ToastMsg['action']; error?: boolean; ms?: number } = {}) {
    const id = ++this.#seq;
    // One message at a time keeps the corner calm; the newest wins.
    this.items = [{ id, text, action: opts.action, error: opts.error }];
    setTimeout(() => this.dismiss(id), opts.ms ?? (opts.action ? 8000 : 4000));
  }

  error(e: unknown) {
    this.show(String(e).replace(/^Error: /, ''), { error: true, ms: 8000 });
  }

  dismiss(id: number) {
    this.items = this.items.filter((t) => t.id !== id);
  }
}

export const toast = new ToastState();
