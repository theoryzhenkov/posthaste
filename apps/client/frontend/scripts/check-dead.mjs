#!/usr/bin/env node
/**
 * knip dead-code ratchet (charter §The ratchet): fail on NEW findings only.
 *
 * Why a wrapper and not knip's ignore lists: ignores are pattern-scoped
 * (whole files, dependency names), so listing today's offenders there would
 * also hide NEW dead exports appearing in the same files. Instead knip runs
 * with its JSON reporter and every finding is flattened to an exact key
 * (file::category::symbol) and diffed against the committed baseline —
 * scripts/knip-baseline.json.
 *
 * The slice-5 sweep burned the 261-entry burn-down list to zero. Anything
 * appearing here now is dead code: delete it, don't baseline it. A knip
 * false positive (none known today) is the only thing that may be listed,
 * and it needs a justifying comment right here.
 *
 * Usage:
 *   node scripts/check-dead.mjs            diff against the baseline
 *   node scripts/check-dead.mjs --update   rewrite the baseline (shrink only:
 *                                          review the diff before committing)
 */
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const root = join(import.meta.dirname, '..')
const baselinePath = join(import.meta.dirname, 'knip-baseline.json')
const update = process.argv.includes('--update')

const raw = execFileSync('bunx', ['knip', '--reporter', 'json', '--no-exit-code'], {
  cwd: root,
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024,
})
const report = JSON.parse(raw)

const keys = []
for (const issue of report.issues ?? []) {
  for (const [category, value] of Object.entries(issue)) {
    if (category === 'file') continue
    if (value === true) {
      keys.push(`${issue.file}::file`)
      continue
    }
    if (!Array.isArray(value)) continue
    for (const item of value) {
      const name = typeof item === 'string' ? item : item.name
      keys.push(`${issue.file}::${category}::${name}`)
    }
  }
}
keys.sort()

if (update) {
  writeFileSync(baselinePath, JSON.stringify(keys, null, 2) + '\n')
  console.log(`knip baseline rewritten: ${keys.length} findings.`)
  process.exit(0)
}

const baseline = new Set(JSON.parse(readFileSync(baselinePath, 'utf8')))
const fresh = keys.filter((k) => !baseline.has(k))
const burned = [...baseline].filter((k) => !keys.includes(k))

if (fresh.length > 0) {
  console.error(`dead-code check failed: ${fresh.length} NEW finding(s):`)
  for (const k of fresh) console.error(`  ${k}`)
  console.error('Delete the dead code (delete beats organize) — do not baseline it.')
  process.exit(1)
}
if (burned.length > 0) {
  console.log(
    `dead-code check passed; ${burned.length} baseline entr(y/ies) resolved — shrink the baseline with --update.`,
  )
} else {
  console.log(`dead-code check passed: no new findings (baseline: ${baseline.size}).`)
}
