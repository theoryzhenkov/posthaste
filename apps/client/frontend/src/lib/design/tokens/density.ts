export const uiDensities = ['compact', 'cozy', 'comfortable'] as const

export type UiDensity = (typeof uiDensities)[number]

export const defaultUiDensity = 'compact' as const satisfies UiDensity

/**
 * Canonical per-density layout dimensions, in pixels.
 *
 * These mirror the `--density-*` custom properties defined per
 * `[data-ui-density]` in `index.css` — keep the two in sync. The CSS vars drive
 * the non-virtualized surfaces (toolbar, sidebar rows, pane padding) via
 * `var(--density-*)`; `messageRowHeight` additionally feeds the message list's
 * hand-rolled virtualizer, which needs a JS number for its spacer/index math.
 */
export const uiDensitySettings = {
  compact: {
    toolbarHeight: 38,
    sidebarRowHeight: 24,
    messageRowHeight: 24,
    panePadding: 10,
  },
  cozy: {
    toolbarHeight: 42,
    sidebarRowHeight: 28,
    messageRowHeight: 30,
    panePadding: 12,
  },
  comfortable: {
    toolbarHeight: 46,
    sidebarRowHeight: 32,
    messageRowHeight: 48,
    panePadding: 16,
  },
} as const satisfies Record<
  UiDensity,
  {
    toolbarHeight: number
    sidebarRowHeight: number
    messageRowHeight: number
    panePadding: number
  }
>

/** The message-list row height (px) for `density` — feeds the virtualizer. */
export function messageRowHeight(density: UiDensity): number {
  return uiDensitySettings[density].messageRowHeight
}

/** Parse a stored/broadcast value into a UI density, or `null` (R3). */
export function parseUiDensity(value: string): UiDensity | null {
  return uiDensities.includes(value as UiDensity) ? (value as UiDensity) : null
}
