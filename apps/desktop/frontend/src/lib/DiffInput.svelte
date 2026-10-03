<script lang="ts">
  let { left = $bindable(''), right = $bindable(''), base = $bindable(''), merge = false }: {
    left: string; right: string; base: string; merge?: boolean;
  } = $props();
</script>

<div class="diff-inputs">
  {#if merge}<label class="base">Original version<textarea bind:value={base} spellcheck="false" placeholder="Common original before either set of changes"></textarea></label>{/if}
  <label>{merge ? 'Your changes' : 'Original text'}<textarea id="tool-input" bind:value={left} spellcheck="false" placeholder={merge ? 'Paste your version' : 'Paste the original text'}></textarea></label>
  <label>{merge ? 'Other changes' : 'Changed text'}<textarea bind:value={right} spellcheck="false" placeholder={merge ? 'Paste the other version' : 'Paste the changed text'}></textarea></label>
</div>

<style>
  .diff-inputs { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; padding: 14px; }
  label { display: grid; gap: 8px; min-width: 0; font-size: 13px; color: var(--muted); }
  .base { grid-column: 1 / -1; }
  textarea { width: 100%; min-width: 0; min-height: 140px; max-height: 260px; resize: vertical; border: 1px solid var(--line); border-radius: 8px; padding: 10px; background: var(--surface); color: var(--ink); font: 13px/1.6 ui-monospace, monospace; }
  .base textarea { min-height: 85px; }
  @media (max-width: 480px) { .diff-inputs { grid-template-columns: minmax(0, 1fr); } }
</style>
