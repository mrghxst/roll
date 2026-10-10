<script lang="ts" generics="T extends string">
  let {
    options,
    value = $bindable(),
    label,
    onchange,
  }: { options: { value: T; label: string }[]; value: T; label: string; onchange?: (v: T) => void } = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <button
      role="radio"
      aria-checked={value === o.value}
      onclick={() => {
        value = o.value;
        onchange?.(o.value);
      }}>{o.label}</button
    >
  {/each}
</div>

<style>
  .seg {
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--surface);
  }
  button {
    height: 26px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--r-1);
    background: transparent;
    color: var(--muted);
    font-size: var(--t13);
    cursor: pointer;
  }
  button[aria-checked='true'] {
    background: var(--bg);
    color: var(--fg);
    box-shadow: 0 0 0 1px var(--border);
  }
</style>
