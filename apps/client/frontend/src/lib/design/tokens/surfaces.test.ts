import { describe, expect, test } from 'bun:test'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import {
  compositedSurfaceRoles,
  contentSurfaceRoles,
  surfaceCustomProperties,
  surfaceRoles,
  surfaceThemes,
} from './surfaces'

/** The only consumer of the surface tokens (`.surface-<role>` rules). */
const CSS_PATH = join(import.meta.dir, '../../../app/assets/index.css')
// Comments are stripped: the assertions below scan for rule shapes, and the
// stylesheet's own prose quotes the very patterns they forbid.
const css = readFileSync(CSS_PATH, 'utf8').replaceAll(/\/\*[\s\S]*?\*\//g, '')

const themeKeys = Object.keys(surfaceThemes) as (keyof typeof surfaceThemes)[]

/** Every `.ts`/`.tsx` file under `dir`, recursively. */
function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.tsx?$/.test(entry.name) ? [path] : []
  })
}

/** Every `.surface-<role> { … }` rule body, keyed by role. */
function surfaceRuleBodies(): Map<string, string> {
  const bodies = new Map<string, string>()
  for (const match of css.matchAll(/\.surface-([a-z]+)\s*\{([^}]*)\}/g)) {
    bodies.set(match[1] as string, match[2] as string)
  }
  return bodies
}

describe('surface theme completeness', () => {
  test('every theme publishes the same token set', () => {
    // The `Record<SurfaceThemeKey, SurfaceTheme>` type already makes an
    // incomplete theme a compile error; this is the runtime mirror of that, so
    // a future data-driven theme source (imported CSS, a user theme file)
    // cannot get in under the type check.
    expect(themeKeys.length).toBe(4)
    const reference = surfaceCustomProperties(surfaceThemes[themeKeys[0]!])
      .map(([property]) => property)
      .sort()
    expect(reference.length).toBeGreaterThan(0)
    for (const key of themeKeys) {
      const published = surfaceCustomProperties(surfaceThemes[key])
      const names = published.map(([property]) => property).sort()
      expect(names, `${key} publishes a different token set`).toEqual(reference)
      for (const [property, value] of published) {
        expect(value.trim(), `${key} ${property} is empty`).not.toBe('')
      }
    }
  })

  test('only the floating tiers declare a blur', () => {
    for (const key of themeKeys) {
      const names = surfaceCustomProperties(surfaceThemes[key]).map(
        ([property]) => property,
      )
      for (const role of contentSurfaceRoles) {
        expect(
          names,
          `${key} gave the content role ${role} a blur`,
        ).not.toContain(`--surface-${role}-blur`)
      }
      for (const role of compositedSurfaceRoles) {
        expect(names).toContain(`--surface-${role}-blur`)
      }
    }
  })
})

describe('surface token CSS drift', () => {
  test('index.css consumes exactly the tokens the themes publish', () => {
    const published = new Set(
      surfaceCustomProperties(surfaceThemes['neutral-light']).map(
        ([property]) => property,
      ),
    )
    const referenced = new Set(
      [...css.matchAll(/var\((--surface-[a-z-]+)/g)].map(
        (match) => match[1] as string,
      ),
    )
    expect(
      [...referenced].sort(),
      'index.css references an unknown surface token',
    ).toEqual([...published].sort())
  })

  test('every role has exactly one rule, and it paints fill/border/shadow', () => {
    const bodies = surfaceRuleBodies()
    expect([...bodies.keys()].sort()).toEqual([...surfaceRoles].sort())
    for (const [role, body] of bodies) {
      expect(body, `.surface-${role} does not set a fill`).toContain(
        `var(--surface-${role}-fill`,
      )
      expect(body, `.surface-${role} does not set a border`).toContain(
        `var(--surface-${role}-border`,
      )
      expect(body, `.surface-${role} does not set a shadow`).toContain(
        `var(--surface-${role}-shadow`,
      )
    }
  })
})

describe('compositing is confined to the floating tiers', () => {
  test('no stylesheet rule outside the floating tiers composites', () => {
    // The live defect this system exists to kill:
    //   [data-palette-preset='glass'] .bg-panel { backdrop-filter: blur(44px) }
    // turned every content pane into a stacking context and a backdrop root,
    // so an in-flow pane could occlude a portaled WINDOW-tier panel it outranks
    // by z-index. Anything that reintroduces a backdrop-filter outside the
    // floating tiers fails here.
    const allowed = new Set(
      compositedSurfaceRoles.map((role) => `.surface-${role}`),
    )
    const rules = [...css.matchAll(/([^{}]*)\{([^{}]*)\}/g)].filter((rule) =>
      (rule[2] as string).includes('backdrop-filter'),
    )
    expect(rules.length).toBe(compositedSurfaceRoles.length)
    for (const rule of rules) {
      for (const selector of (rule[1] as string).split(',')) {
        expect(
          allowed.has(selector.trim()),
          `"${selector.trim()}" declares backdrop-filter outside the floating tiers`,
        ).toBe(true)
      }
    }
  })

  test('no theme selector reaches into a utility class', () => {
    // Themes declare material as data. A rule that keys off a palette attribute
    // AND names a class is a theme rewriting what a utility means — the exact
    // shape that produced the bug above. `.dark` is the mode carrier, not a
    // target, so it is stripped before the check.
    for (const rule of css.matchAll(/([^{}]*)\{/g)) {
      const selector = (rule[1] as string).trim()
      if (!selector.includes('data-palette-')) continue
      expect(
        /\.[a-z]/.test(selector.replaceAll('.dark', '')),
        `theme selector "${selector}" targets a class`,
      ).toBe(false)
    }
  })

  test('no component composites a content-role surface', () => {
    // The component-side half: a content role must never gain a blur by having
    // `backdrop-blur-*` pasted next to it in a className.
    const roleClasses = contentSurfaceRoles.map((role) => `surface-${role}`)
    for (const file of sourceFiles(join(import.meta.dir, '../../..'))) {
      const source = readFileSync(file, 'utf8')
      for (const literal of source.matchAll(/(['"`])([^'"`\n]*)\1/g)) {
        const text = literal[2] as string
        const role = roleClasses.find((candidate) => text.includes(candidate))
        if (role === undefined) continue
        expect(
          /backdrop-(blur|filter)/.test(text),
          `${file} composites the content role ${role}`,
        ).toBe(false)
      }
    }
  })
})
