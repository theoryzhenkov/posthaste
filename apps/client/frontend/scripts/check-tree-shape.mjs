#!/usr/bin/env node
/**
 * R10 folder-shape check (docs/client/L2-charter.md): every folder under
 * src/ holds at most 8 entries (files + subfolders); non-leaf folders hold
 * at least 3; no folder holds fewer than 2 (no single-child folders).
 * Tests (*.test.*) colocate with their source and do not count. gen/ is
 * generated and exempt entirely.
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

const failures = []

function walk(dir) {
  const entries = readdirSync(dir, { withFileTypes: true })
  const dirs = entries.filter(
    (e) => e.isDirectory() && !(dir === root && e.name === 'gen'),
  )
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
  for (const d of dirs) walk(join(dir, d.name))
}

walk(root)

if (failures.length > 0) {
  console.error('folder-shape check failed (R10, 3-8 entries per folder):')
  for (const f of failures) console.error(`  ${f}`)
  process.exit(1)
}
console.log('folder-shape check passed: every folder within the 3-8 budget.')
