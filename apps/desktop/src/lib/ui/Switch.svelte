<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    onchange,
  }: { checked?: boolean; label: string; onchange?: (v: boolean) => void } = $props();
</script>

<button
  class="switch"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  onclick={() => {
    checked = !checked;
    onchange?.(checked);
  }}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 32px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 9px;
    background: var(--surface);
    cursor: pointer;
    transition: background 150ms var(--ease);
  }
  .switch[aria-checked='true'] {
    background: var(--fg);
    border-color: var(--fg);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 150ms var(--ease);
  }
  .switch[aria-checked='true'] .knob {
    transform: translateX(14px);
    background: var(--bg);
  }
</style>
