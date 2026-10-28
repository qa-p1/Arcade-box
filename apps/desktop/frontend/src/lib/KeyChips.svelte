<script lang="ts">
  // Read-only keys as key chips, as Arcade Tools shows them. Each entry is
  // one chord; "Ctrl+K" shows as two keys and entries are joined by "or".
  import { chordParts } from './chords';

  let { keys }: { keys: string[] } = $props();
  const chords = $derived(keys.map((chord) => (chord.length === 1 ? [chord] : chordParts(chord))));
</script>

<span class="key-chip-list" aria-label={`Keys: ${keys.join(' or ')}`}>
  {#each chords as parts, index (`${keys[index]}-${index}`)}
    {#if index > 0}<span class="key-or">or</span>{/if}
    {#each parts as part, partIndex (`${part}-${partIndex}`)}<kbd class="key-chip">{part}</kbd>{/each}
  {/each}
</span>
