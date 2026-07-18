export const themeModes = ['light', 'dark', 'system'] as const
export const resolvedThemeModes = ['light', 'dark'] as const

export type ThemeMode = (typeof themeModes)[number]
export type ResolvedThemeMode = (typeof resolvedThemeModes)[number]

export const defaultThemeMode = 'dark' as const satisfies ThemeMode

/**
 * Themes are identified by a free-form string id, not a fixed enum — so
 * user-supplied themes (a future imported-CSS feature) need no schema change.
 * The renderer ships two built-ins; an unknown id renders as the default.
 */
export type BuiltInThemeId = 'neutral' | 'glass'

/**
 * `style` is the value written to `data-palette-style` (and mirrored to
 * `data-palette-preset`), which the CSS keys off for structural looks. Only
 * `glass` has structural CSS today; `neutral` is the unadorned base.
 */
export type ThemeStyle = 'neutral' | 'glass'

export type ThemeDefinition = {
  readonly id: BuiltInThemeId
  readonly label: string
  readonly description: string
  readonly style: ThemeStyle
}

export const builtInThemes = {
  neutral: {
    id: 'neutral',
    label: 'Classic',
    description: 'The clean, solid Posthaste look — fully recolorable',
    style: 'neutral',
  },
  glass: {
    id: 'glass',
    label: 'Glass',
    description: 'Layered translucent panes over a customizable color wash',
    style: 'glass',
  },
} as const satisfies Record<BuiltInThemeId, ThemeDefinition>

export const builtInThemeIds = ['neutral', 'glass'] as const

export const defaultThemeId = 'neutral' as const satisfies BuiltInThemeId

/** Parse a stored/broadcast value into a theme mode, or `null` (R3). */
export function parseThemeMode(value: string): ThemeMode | null {
  return themeModes.includes(value as ThemeMode) ? (value as ThemeMode) : null
}

/** Internal guard behind {@link themeStyle}; not exported (R3). */
function isBuiltInThemeId(value: string): value is BuiltInThemeId {
  return (builtInThemeIds as readonly string[]).includes(value)
}

/**
 * The `data-palette-style` for a theme id. Unknown ids (e.g. a future user
 * theme) fall back to the neutral base style.
 */
export function themeStyle(themeId: string): ThemeStyle {
  return isBuiltInThemeId(themeId) ? builtInThemes[themeId].style : 'neutral'
}
