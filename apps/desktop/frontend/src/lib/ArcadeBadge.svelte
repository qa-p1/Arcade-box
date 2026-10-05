<script lang="ts">
  import { arcadeGlyphs } from './arcade-glyphs';
  import tokens from './arcade-tokens.json';
  let { app }: { app: string } = $props();
</script>

<span class="arcade-badge" style:color={(tokens.accent as Record<string, string>)[app]} aria-hidden="true">{@html arcadeGlyphs[app] ?? ''}</span>

<style>
  .arcade-badge { width: 16px; height: 16px; display: inline-flex; flex-shrink: 0; }
  .arcade-badge :global(svg) { width: 16px; height: 16px; }
</style>
