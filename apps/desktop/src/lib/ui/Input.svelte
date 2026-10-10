<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';

  type Props = HTMLInputAttributes & { value?: string | number | null; mono?: boolean; el?: HTMLInputElement };

  let { value = $bindable(), mono = false, el = $bindable(), class: cls = '', ...rest }: Props = $props();
</script>

<input class="input {cls}" class:mono bind:value bind:this={el} spellcheck="false" {...rest} />

<style>
  .input {
    height: 32px;
    width: 100%;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-btn);
    background: var(--bg);
    font-size: var(--t13);
    outline: none;
    transition: border-color 120ms var(--ease);
    user-select: text;
    -webkit-user-select: text;
  }
  .input:hover {
    border-color: var(--border-strong);
  }
  .input:focus {
    border-color: var(--fg);
  }
  .input::placeholder {
    color: var(--muted);
  }
</style>
