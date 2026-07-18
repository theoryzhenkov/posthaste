/**
 * Client charter slice 1 — moves-only restructure, driven by restructure-manifest.txt.
 *
 * Textual, memory-sane executor (the earlier ts-morph attempt loaded the whole
 * project graph and exhausted the VM). Two phases:
 *   1. `rename` every manifest entry on disk (target dirs created; entries
 *      already at their destination are skipped).
 *   2. Rewrite import specifiers TEXTUALLY in every src file from the manifest
 *      mapping: `@/<old>` alias specifiers are looked up directly; relative
 *      specifiers are resolved against the importing file's OLD directory,
 *      mapped, and re-relativized against its NEW directory. Pure path math,
 *      no type resolution. tsc afterwards is the net for anything missed.
 *
 * Run from the frontend root: bun scripts/restructure.ts
 * Idempotent: re-running after completion is a no-op.
 */
import { readFileSync, writeFileSync, mkdirSync, renameSync, existsSync, readdirSync } from 'node:fs'
import { dirname, join, resolve, posix } from 'node:path'

const frontendRoot = resolve(import.meta.dirname, '..')
const srcRoot = join(frontendRoot, 'src')
const manifestPath = join(import.meta.dirname, 'restructure-manifest.txt')

// ---- manifest: old src-relative path -> new src-relative path ----
const mapping = new Map<string, string>()
for (const raw of readFileSync(manifestPath, 'utf8').split('\n')) {
  const line = raw.trim()
  if (line === '' || line.startsWith('#')) continue
  const [from, to] = line.split(' -> ').map((s) => s.trim())
  if (!from || !to) throw new Error(`bad manifest line: ${raw}`)
  if (mapping.has(from)) throw new Error(`duplicate manifest source: ${from}`)
  mapping.set(from, to)
}

// ---- phase 1: move files ----
let moved = 0
for (const [from, to] of mapping) {
  if (from === to) continue
  const fromAbs = join(srcRoot, from)
  const toAbs = join(srcRoot, to)
  if (!existsSync(fromAbs)) {
    if (!existsSync(toAbs)) throw new Error(`missing on both ends: ${from} -> ${to}`)
    continue // already moved (resumed run)
  }
  mkdirSync(dirname(toAbs), { recursive: true })
  renameSync(fromAbs, toAbs)
  moved += 1
}
console.log(`moved ${moved} files`)

// ---- phase 2: rewrite import specifiers ----
const CODE_EXT = /\.(ts|tsx)$/

/** Resolve a src-relative module path (no leading src/) to its OLD file entry
 * in the mapping, returning [oldFile, style] where style records how the
 * specifier addressed it (bare = extensionless ts/tsx, exact = with
 * extension, index = directory import). */
function resolveOld(rel: string): [string, 'bare' | 'exact' | 'index'] | null {
  for (const ext of ['.ts', '.tsx']) if (mapping.has(rel + ext)) return [rel + ext, 'bare']
  if (mapping.has(rel)) return [rel, 'exact']
  for (const ext of ['/index.ts', '/index.tsx']) if (mapping.has(rel + ext)) return [rel + ext, 'index']
  return null
}

/** New specifier path (src-relative, no leading src/) for a mapped target. */
function newSpecPath(newFile: string, style: 'bare' | 'exact' | 'index', forAlias: boolean): string {
  if (style === 'exact') return newFile
  let p = newFile.replace(CODE_EXT, '')
  // Alias imports drop a trailing /index (matches house style); relative
  // imports keep it explicit so './index' in-dir stays well-formed.
  if (forAlias && p.endsWith('/index')) p = p.slice(0, -'/index'.length)
  return p
}

const SPEC_RE = /(\bfrom\s*|\bimport\s*\(\s*|\bimport\s+)(['"])([^'"\n]+)\2/g

const newToOld = new Map<string, string>()
for (const [from, to] of mapping) newToOld.set(to, from)

let rewrittenFiles = 0
let rewrittenSpecs = 0
const unresolved: string[] = []

function walk(dir: string): string[] {
  const out: string[] = []
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.name === 'gen' && dir === srcRoot) continue
    const p = join(dir, e.name)
    if (e.isDirectory()) out.push(...walk(p))
    else out.push(p)
  }
  return out
}

for (const abs of walk(srcRoot)) {
  if (!CODE_EXT.test(abs)) continue
  const newRel = posix.normalize(abs.slice(srcRoot.length + 1))
  const oldRel = newToOld.get(newRel) ?? newRel
  const oldDir = posix.dirname(oldRel)
  const newDir = posix.dirname(newRel)
  const before = readFileSync(abs, 'utf8')
  const after = before.replace(SPEC_RE, (whole, lead: string, q: string, spec: string) => {
    let resolved: [string, 'bare' | 'exact' | 'index'] | null = null
    let alias = false
    if (spec.startsWith('@/')) {
      alias = true
      resolved = resolveOld(spec.slice(2))
    } else if (spec.startsWith('.')) {
      resolved = resolveOld(posix.normalize(posix.join(oldDir, spec)))
    } else {
      return whole // external package
    }
    if (!resolved) {
      unresolved.push(`${newRel}: ${spec}`)
      return whole
    }
    const [oldFile, style] = resolved
    const target = newSpecPath(mapping.get(oldFile)!, style, alias)
    let next: string
    if (alias) {
      next = `@/${target}`
    } else {
      next = posix.relative(newDir, target)
      if (!next.startsWith('.')) next = `./${next}`
    }
    if (next !== spec) rewrittenSpecs += 1
    return `${lead}${q}${next}${q}`
  })
  if (after !== before) {
    writeFileSync(abs, after)
    rewrittenFiles += 1
  }
}

console.log(`rewrote ${rewrittenSpecs} specifiers across ${rewrittenFiles} files`)
if (unresolved.length > 0) {
  console.warn(`unresolved internal-looking specifiers (verify by hand):`)
  for (const u of unresolved) console.warn(`  ${u}`)
}
