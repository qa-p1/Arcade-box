import type { IconName as KitIconName } from '../design/icons';

/** Box's names for kit icons that the kit calls something else. */
const ALIASES = {
  spark: 'sparkle',
  grid: 'apps',
  arrow: 'arrowRight',
  close: 'x',
  chevron: 'chevronRight',
  back: 'arrowLeft',
  dots: 'more',
  network: 'link',
} as const satisfies Record<string, KitIconName>;

export type IconName = KitIconName | keyof typeof ALIASES;

export function kitName(name: IconName): KitIconName {
  return (ALIASES as Record<string, KitIconName>)[name] ?? (name as KitIconName);
}
