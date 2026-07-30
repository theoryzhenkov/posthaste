/**
 * Surface roles: the closed set of MATERIALS the shell paints regions with.
 *
 * Before this module a theme was a bag of colours and the *material* — opacity,
 * blur, elevation — was smuggled in two incompatible ways: as alpha baked into
 * a colour token (`--panel: rgba(18,16,26,0.45)`), and as a palette-scoped
 * selector that reached into Tailwind's utility classes and changed what they
 * mean:
 *
 *   [data-palette-preset='glass'] .bg-panel { backdrop-filter: blur(44px) … }
 *
 * That second form is the one that hurt. `backdrop-filter` promotes its element
 * to a composited layer, makes it a stacking context AND a backdrop root — so
 * every content pane in the app silently became one, but only under glass. A
 * floating panel portaled to `document.body` at the WINDOW tier could then be
 * occluded by an in-flow pane it outranks by every plain-CSS rule.
 *
 * The fix is to make that unrepresentable rather than merely discouraged:
 *
 *  1. A theme is DATA (this file), never a selector. Nothing here can target a
 *     `.bg-*` class, because nothing here emits CSS selectors at all — the
 *     materials are written to the root element as custom properties by
 *     `applyRootTheme`, and `index.css` has exactly one `.surface-<role>` rule
 *     per role that consumes them.
 *  2. Only the FLOATING tiers have a `blur` facet at all. A content role has no
 *     field to put a `backdrop-filter` in, so a theme cannot composite a pane
 *     even if its author wants to. Content roles get translucency from fill
 *     alpha instead — which costs nothing and stacks predictably.
 *
 * Adding a theme is adding a key to {@link surfaceThemes}: the `Record` type
 * makes an incomplete theme a compile error, so the old "a missing token
 * silently inherits from `:root`, which is the LIGHT theme" hazard cannot
 * recur. `surfaces.test.ts` guards the other half — that `index.css` consumes
 * exactly the tokens this file produces, and composites nothing else.
 */
import type { ResolvedThemeMode, ThemeStyle } from '../theme/theme'

/**
 * Roles that tile the shell. They are in the document flow, may nest inside one
 * another, and must never establish a stacking context of their own.
 */
export const contentSurfaceRoles = [
  'chrome',
  'sidebar',
  'pane',
  'card',
] as const

/**
 * Roles that float above the shell. Each is portaled out of the flow and
 * positioned against the viewport, so compositing one costs a single layer and
 * cannot reorder anything below it.
 */
export const compositedSurfaceRoles = ['floating', 'overlay'] as const

export const surfaceRoles = [
  ...contentSurfaceRoles,
  ...compositedSurfaceRoles,
] as const

type ContentSurfaceRole = (typeof contentSurfaceRoles)[number]
type CompositedSurfaceRole = (typeof compositedSurfaceRoles)[number]
type SurfaceRole = ContentSurfaceRole | CompositedSurfaceRole

/** What every role declares: a paint, a hairline, and an elevation. */
type ContentMaterial = {
  /** Any `background` value — a colour, or a gradient for tinted surfaces. */
  readonly fill: string
  readonly border: string
  /** `box-shadow` value; `none` for surfaces that tile rather than float. */
  readonly shadow: string
}

/** Floating tiers additionally declare their `backdrop-filter`. */
type CompositedMaterial = ContentMaterial & {
  readonly blur: string
}

export type SurfaceTheme = {
  readonly [Role in ContentSurfaceRole]: ContentMaterial
} & {
  readonly [Role in CompositedSurfaceRole]: CompositedMaterial
}

/**
 * A theme is identified by (structural style × resolved mode). `system` is
 * resolved before it gets here, so the matrix is total and small.
 */
export type SurfaceThemeKey = `${ThemeStyle}-${ResolvedThemeMode}`

/**
 * The brand wash that marks a surface as "this floats above your work".
 * Composed from the live accent tokens (`applyRootTheme` rewrites those on
 * every recolour) so floating surfaces re-tint with the rest of the app.
 */
function accentWash(base: string): string {
  return (
    `linear-gradient(135deg,` +
    ` color-mix(in oklab, var(--brand-coral) 14%, ${base}) 0%,` +
    ` color-mix(in oklab, var(--ring) 7%, ${base}) 50%,` +
    ` ${base} 100%)`
  )
}

/** The hairline that goes with {@link accentWash}. */
const accentHairline =
  'color-mix(in oklab, var(--brand-coral) 22%, var(--border))'

/**
 * The solid themes take their materials straight from the palette: the fills
 * are opaque, so there is nothing for a blur to reveal and both floating tiers
 * declare `none`. Only the elevation differs between light and dark.
 */
function solidSurfaces(shadow: string, popoverShadow: string): SurfaceTheme {
  return {
    chrome: {
      fill: 'var(--chrome)',
      border: 'var(--border-soft)',
      shadow: 'none',
    },
    sidebar: {
      fill: 'var(--sidebar)',
      border: 'var(--sidebar-border)',
      shadow: 'none',
    },
    pane: { fill: 'var(--panel)', border: 'var(--border)', shadow: 'none' },
    card: { fill: 'var(--card)', border: 'var(--border)', shadow: 'none' },
    floating: {
      fill: accentWash('var(--panel)'),
      border: accentHairline,
      shadow,
      blur: 'none',
    },
    overlay: {
      fill: accentWash('var(--popover)'),
      border: accentHairline,
      shadow: popoverShadow,
      blur: 'none',
    },
  }
}

/**
 * What distinguishes one glass mode from the other: four fills and two
 * hairlines over the mesh wash, plus its elevation.
 *
 * The fills are deliberately more opaque than the pre-role glass palette was
 * (chrome .60 → .72, sidebar .55 → .66, pane .70 → .82). The old values leaned
 * on a 44px `backdrop-filter` per pane to stay legible; panes no longer carry
 * one, so the alpha has to do that work alone.
 */
type GlassTint = {
  readonly chrome: string
  readonly sidebar: string
  readonly pane: string
  readonly card: string
  readonly hairline: string
  readonly cardHairline: string
  readonly shadow: string
  readonly popoverShadow: string
}

/**
 * Glass composites the floating tiers only. Overlays sit on `--popover` (the
 * most opaque palette surface) rather than `--panel`: a command palette must
 * stay readable over an arbitrarily busy shell, and it is the one surface the
 * user reads while everything behind it is still moving.
 */
function glassSurfaces(tint: GlassTint): SurfaceTheme {
  return {
    chrome: { fill: tint.chrome, border: tint.hairline, shadow: 'none' },
    sidebar: { fill: tint.sidebar, border: tint.hairline, shadow: 'none' },
    pane: { fill: tint.pane, border: tint.hairline, shadow: 'none' },
    card: { fill: tint.card, border: tint.cardHairline, shadow: 'none' },
    floating: {
      fill: accentWash('var(--panel)'),
      border: accentHairline,
      shadow: tint.shadow,
      blur: 'blur(44px) saturate(180%)',
    },
    overlay: {
      fill: accentWash('var(--popover)'),
      border: accentHairline,
      shadow: tint.popoverShadow,
      blur: 'blur(44px) saturate(180%)',
    },
  }
}

/**
 * The theme matrix. `Record<SurfaceThemeKey, …>` is the completeness guarantee:
 * a new structural style forces four entries and a missing role or facet is a
 * type error, so no theme can half-inherit another's material.
 */
export const surfaceThemes = {
  'neutral-light': solidSurfaces(
    '0 28px 80px rgb(0 0 0 / 0.24)',
    'var(--shadow-popover)',
  ),
  'neutral-dark': solidSurfaces(
    '0 28px 80px rgb(0 0 0 / 0.48)',
    'var(--shadow-popover)',
  ),
  'glass-light': glassSurfaces({
    chrome: 'rgba(255, 255, 255, 0.72)',
    sidebar: 'rgba(255, 255, 255, 0.66)',
    pane: 'rgba(255, 255, 255, 0.82)',
    card: 'rgba(255, 255, 255, 0.66)',
    hairline: 'rgba(0, 0, 0, 0.06)',
    cardHairline: 'rgba(0, 0, 0, 0.1)',
    shadow: '0 28px 80px rgba(40, 30, 60, 0.28)',
    popoverShadow: 'var(--shadow-popover)',
  }),
  'glass-dark': glassSurfaces({
    chrome: 'rgba(15, 13, 22, 0.7)',
    sidebar: 'rgba(20, 18, 30, 0.58)',
    pane: 'rgba(18, 16, 26, 0.64)',
    card: 'rgba(20, 18, 30, 0.58)',
    hairline: 'rgba(255, 255, 255, 0.07)',
    cardHairline: 'rgba(255, 255, 255, 0.12)',
    shadow: '0 28px 80px rgba(0, 0, 0, 0.55)',
    popoverShadow: 'var(--shadow-popover)',
  }),
} as const satisfies Record<SurfaceThemeKey, SurfaceTheme>

export function surfaceThemeFor(
  style: ThemeStyle,
  mode: ResolvedThemeMode,
): SurfaceTheme {
  return surfaceThemes[`${style}-${mode}`]
}

/** The custom property a role's facet is published under. */
function surfaceTokenName(role: SurfaceRole, facet: string): string {
  return `--surface-${role}-${facet}`
}

/**
 * Flatten a theme into the `[property, value]` pairs `applyRootTheme` writes to
 * the root element. Facets are read off the material object rather than a fixed
 * list, so the content/composited split above is what decides whether a role
 * publishes a `blur` at all — there is no second place to keep in step.
 */
export function surfaceCustomProperties(
  theme: SurfaceTheme,
): readonly (readonly [string, string])[] {
  return surfaceRoles.flatMap((role) =>
    Object.entries(theme[role] as Record<string, string>).map(
      ([facet, value]) => [surfaceTokenName(role, facet), value] as const,
    ),
  )
}
