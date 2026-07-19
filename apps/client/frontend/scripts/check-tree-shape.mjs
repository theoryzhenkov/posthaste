#!/usr/bin/env node
/**
 * R10 folder-shape check (docs/client/L2-charter.md): every folder under
 * src/ holds at most 8 entries (files + subfolders); non-leaf folders hold
 * at least 3; no folder holds fewer than 2 (no single-child folders).
 * Tests (*.test.*) colocate with their source and do not count. Generated
 * wire types live in apps/client/protocol, outside the scanned tree.
 *
 * Also enforces R6 naming: no file or folder basename of utils/helpers/misc
 * -- shared helpers live in lib/<domain>.ts named by what they operate on.
 * Zero offenders today, so R6 has no baseline; any hit is a NEW violation.
 *
 * Usage: node scripts/check-tree-shape.mjs [srcDir]
 * Exits 1 listing every folder outside the budget.
 */
import { readdirSync } from 'node:fs'
import { join, relative } from 'node:path'

const root = process.argv[2] ?? join(import.meta.dirname, '..', 'src')
const MAX_ENTRIES = 8
const MIN_NON_LEAF = 3
const MIN_ANY = 2
const BANNED_NAMES = /^(utils|helpers|misc)(\.[^.]+)*$/i

const failures = []

function walk(dir) {
  const entries = readdirSync(dir, { withFileTypes: true })
  const dirs = entries.filter((e) => e.isDirectory())
  const files = entries.filter(
    (e) => e.isFile() && !/\.test\.[^.]+$/.test(e.name),
  )
  const counted = dirs.length + files.length
  const label = relative(root, dir) || 'src'
  if (counted > MAX_ENTRIES) {
    failures.push(`${label}: ${counted} entries (max ${MAX_ENTRIES})`)
  } else if (dirs.length > 0 && counted < MIN_NON_LEAF) {
    failures.push(`${label}: ${counted} entries (non-leaf min ${MIN_NON_LEAF})`)
  } else if (dir !== root && counted < MIN_ANY) {
    failures.push(`${label}: ${counted} entries (min ${MIN_ANY})`)
  }
  for (const e of [...dirs, ...files]) {
    if (BANNED_NAMES.test(e.name)) {
      failures.push(
        `${join(label, e.name)}: banned name (R6: no utils/helpers/misc)`,
      )
    }
  }
  for (const d of dirs) walk(join(dir, d.name))
}

walk(root)

if (failures.length > 0) {
  console.error('tree-shape check failed (R10 3-8 entries, R6 naming):')
  for (const f of failures) console.error(`  ${f}`)
  process.exit(1)
}
console.log('tree-shape check passed: 3-8 budget (R10) and naming (R6) clean.')
