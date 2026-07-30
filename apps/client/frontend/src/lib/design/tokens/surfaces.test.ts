import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
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
const css = readFileSync(CSS_PATH, 'utf8')

const themeKeys = Object.keys(surfaceThemes) as (keyof typeof surfaceThemes)[]

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
