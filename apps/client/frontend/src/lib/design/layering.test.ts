import { describe, expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { Z } from './layering'

/** The CSS mirror of the layering scale (`--z-base` … `--z-tooltip`). */
const CSS_PATH = join(import.meta.dir, '../../app/assets/index.css')

describe('layering scale CSS drift', () => {
  test('every Z tier is mirrored as a --z-* custom property with the same value', () => {
    const css = readFileSync(CSS_PATH, 'utf8')
    for (const [tier, value] of Object.entries(Z)) {
      const property = `--z-${tier.toLowerCase()}`
      const match = css.match(new RegExp(`${property}:\\s*(\\d+)\\s*;`))
      expect(match?.[1], `${property} missing from index.css`).toBeDefined()
      expect(Number(match?.[1]), `${property} drifted from Z.${tier}`).toBe(
        value,
      )
    }
  })
})
