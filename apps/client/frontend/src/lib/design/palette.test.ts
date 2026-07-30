import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

/**
 * The palette is still four hand-maintained CSS blocks. Unlike the surface
 * materials (`tokens/surfaces.ts`, typed data with a `Record` completeness
 * check) there is no compiler standing behind them, and the cascade makes an
 * omission SILENT: a token a theme block forgets simply resolves from an
 * earlier block. The specificity is flat — `:root`, `.dark` and
 * `[data-palette-preset='glass']` are all (0,1,0), so source order alone
 * decides — which means a glass-dark token missing from every glass block falls
 * back to `.dark`, and one missing from `.dark` too falls all the way back to
 * `:root`, the LIGHT theme. Nothing fails; the theme just looks half-finished.
 *
 * This file is the missing compiler. Every mode-scoped token must be declared
 * by the theme's OWN block, so adding a theme is a complete piece of work
 * rather than a partial diff against whatever happens to sit above it.
 */
const CSS_PATH = join(import.meta.dir, '../../app/assets/index.css')
const css = readFileSync(CSS_PATH, 'utf8')

/**
 * The four (style × mode) blocks, in cascade order. Each must be self-complete
 * for the tokens the theme is responsible for.
 */
const THEME_BLOCKS = [
  { label: 'neutral-light', selector: ':root' },
  { label: 'neutral-dark', selector: '.dark' },
  { label: 'glass-light', selector: "[data-palette-preset='glass']" },
  { label: 'glass-dark', selector: ".dark[data-palette-preset='glass']" },
] as const

/**
 * Not palette: geometry and scale that no theme varies. These belong to `:root`
 * alone — a theme block redeclaring one is a different smell (a theme reaching
 * past colour into layout), so they are asserted absent rather than required.
 */
const STRUCTURAL_TOKENS = [
  '--z-base',
  '--z-raised',
  '--z-surface',
  '--z-popover',
  '--z-window',
  '--z-overlay',
  '--z-modal',
  '--z-toast',
  '--z-tooltip',
  '--ph-font-size-meta',
  '--ph-font-size-ui',
  '--ph-font-size-body',
  '--ph-font-size-emphasis',
  '--ph-font-size-heading',
  '--ph-icon-xs',
  '--ph-icon-sm',
  '--ph-icon-md',
  '--ph-icon-lg',
  '--density-toolbar-height',
  '--density-sidebar-row-height',
  '--density-message-row-height',
  '--density-pane-padding',
] as const

/**
 * Tokens exempt from per-theme completeness, each for a stated reason. Adding a
 * row here is the escape hatch; it costs a justification, which is the point.
 */
const EXEMPT_TOKENS: Readonly<Record<string, string>> = {
  // Corner radius is a property of the STYLE, not the mode: glass is rounder
  // than neutral in both modes, so glass-dark inheriting glass-light's value is
  // the intended sharing rather than an omission.
  '--radius': 'style-scoped: shared by both modes of a style',
  // Surface hue/chroma parameterize the neutral oklch ramps. The glass palette
  // is written as literals and reads neither, and `applyRootTheme` writes both
  // inline on every apply regardless of theme — the CSS values are a
  // first-frame fallback for the solid themes only.
  '--ph-surface-hue': 'runtime-owned; unused by the glass literals',
  '--ph-surface-chroma': 'runtime-owned; unused by the glass literals',
}

/** The `--token: value;` declarations inside one top-level block. */
function blockTokens(selector: string): ReadonlySet<string> {
  const start = css.indexOf(`\n${selector} {`)
  expect(start, `${selector} block not found in index.css`).toBeGreaterThan(-1)
  const end = css.indexOf('\n}\n', start)
  const body = css.slice(start, end)
  return new Set(
    [...body.matchAll(/^\s*(--[a-z0-9-]+):/gm)].map(
      (match) => match[1] as string,
    ),
  )
}

const blocks = new Map(
  THEME_BLOCKS.map((theme) => [theme.label, blockTokens(theme.selector)]),
)

/** What every theme owes: `:root`'s tokens, less structure and exemptions. */
const requiredTokens = [...(blocks.get('neutral-light') as Set<string>)].filter(
  (token) =>
    !(STRUCTURAL_TOKENS as readonly string[]).includes(token) &&
    !(token in EXEMPT_TOKENS),
)

describe('palette block completeness', () => {
  test('the baseline theme carries a real palette', () => {
    // Guards the guard: if the `:root` parse ever silently yields nothing, every
    // assertion below passes vacuously.
    expect(requiredTokens.length).toBeGreaterThan(40)
    expect(requiredTokens).toContain('--background')
    expect(requiredTokens).toContain('--panel')
  })

  test('every theme block declares every mode-scoped token', () => {
    for (const [label, declared] of blocks) {
      const missing = requiredTokens.filter((token) => !declared.has(token))
      expect(
        missing,
        `${label} inherits ${missing.join(', ')} from an earlier block — ` +
          `declare them or add a justified row to EXEMPT_TOKENS`,
      ).toEqual([])
    }
  })

  test('no theme block redeclares a structural token', () => {
    for (const [label, declared] of blocks) {
      if (label === 'neutral-light') continue
      const structural = STRUCTURAL_TOKENS.filter((token) =>
        declared.has(token),
      )
      expect(
        structural,
        `${label} redeclares structural token(s) ${structural.join(', ')}`,
      ).toEqual([])
    }
  })

  test('every exemption is documented and still real', () => {
    for (const [token, reason] of Object.entries(EXEMPT_TOKENS)) {
      expect(reason.length, `${token} exemption has no reason`).toBeGreaterThan(
        20,
      )
      expect(
        blocks.get('neutral-light')?.has(token),
        `${token} is exempted but no longer exists`,
      ).toBe(true)
    }
  })
})
