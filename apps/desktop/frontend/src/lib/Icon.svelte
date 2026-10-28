<script lang="ts">
  // The kit's outline icons (design/icons.ts). Box's older names map onto
  // the kit names, so call sites keep reading as before.
  import { icons, filledIcons, ICON_STROKE, type IconName as KitIconName } from '../design/icons';
  import { kitName, type IconName } from './icon-names';

  let { name, size = 16 }: { name: IconName; size?: number } = $props();
  const kit = $derived<KitIconName>(kitName(name));
  const filled = $derived(filledIcons.has(kit));
</script>

<svg class="icon" aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill={filled ? 'currentColor' : 'none'} stroke={filled ? 'none' : 'currentColor'} stroke-width={ICON_STROKE} stroke-linecap="round" stroke-linejoin="round"><path d={icons[kit]} /></svg>
