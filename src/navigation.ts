import type { IconName } from './icons';

/**
 * The surfaces the shell can show. The rail is the only navigator, so the view
 * list, its order and its labels live here and are read by both the rail and the
 * stage header.
 */
export type View = 'overview' | 'settings';

/** Rail order. */
export const VIEW_ORDER: readonly View[] = ['overview', 'settings'];

/** Rail icon and label per view; the label doubles as the stage title. */
export const VIEW_NAV: Record<View, { icon: IconName; label: string }> = {
  overview: { icon: 'house', label: '概览' },
  settings: { icon: 'gear', label: '设置' },
};
