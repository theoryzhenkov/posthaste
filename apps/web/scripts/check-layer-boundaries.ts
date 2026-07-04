/**
 * Enforced LAYER boundaries (D119 promotion of the hand-rolled boundary checks).
 *
 * The repo already guards two seams with hand-rolled AST-lite scripts
 * (`check-query-boundaries`, `check-runtime-boundaries`). Those are solid, so —
 * per the M48 directive — this EXTENDS the same approach with the D115/D117
 * structural seams rather than pulling in a new eslint boundaries plugin (zero
 * new deps). Each rule forbids an import whose DIRECTION would invert a layer:
 *
 *  - D115 — the reactive store is the DUMB mirror: `src/live-store/**` must not
 *    import react-query or the entity-store adapter / runtime replica / near-end.
 *    Producers import the store; the store never imports a producer.
 *  - D117 — components are renderers: `src/components/**` must not reach into
 *    runtime replica/adapter/near-end internals (they consume hooks + the store,
 *    not the plumbing).
 *  - D116/D117 — the domain-cache is request/response glue: `src/domain-cache/**`
 *    must not import the entity-store adapter (live state flows through the
 *    store, not the cache).
 *
 * Type-only imports count: a layer that names another layer's types is still
 * coupled to its shape. Exceptions are declared explicitly and narrowly.
 *
 * @spec docs/eph/RFC-L2-client-resilience.md#D119
 * @spec docs/eph/RFC-L2-client-resilience.md#D115
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const sourceRoot = join(root, 'src')

interface LayerRule {
  /** Human name for the seam, used in the error message. */
  name: string
  /** Files this rule governs (relative to `src`, POSIX). */
  appliesTo: (rel: string) => boolean
  /** Import specifiers that violate the rule. */
  forbids: RegExp[]
  /** Files exempt from the rule (relative to web root, POSIX). */
  exempt?: Set<string>
  /** What the author should do instead. */
  remedy: string
}

const RULES: LayerRule[] = [
  {
    name: 'D115 live-store purity',
    appliesTo: (rel) => rel.startsWith('src/live-store/'),
    forbids: [
      /@tanstack\/react-query/,
      /(?:^|\/)runtime\/replica\//,
      /(?:^|\/)runtime\/adapter(?:\.ts)?$/,
      /(?:^|\/)runtime\/nearEnd(?:\.ts)?$/,
      /entityStoreAdapter/,
    ],
    remedy:
      'the reactive store is the dumb mirror (D115): producers import the store, ' +
      'never the reverse. Keep it to types only.',
  },
  {
    name: 'D117 components-vs-runtime-internals',
    appliesTo: (rel) => rel.startsWith('src/components/'),
    forbids: [
      /(?:^|\/)runtime\/replica\//,
      /(?:^|\/)runtime\/nearEnd(?:\.ts)?$/,
      /entityStoreAdapter/,
    ],
    remedy:
      'components render (D117): consume runtime intent facades, hooks, and the ' +
      'live store — not the replica/adapter/near-end plumbing.',
  },
  {
    name: 'D116/D117 domain-cache-vs-adapter',
    appliesTo: (rel) => rel.startsWith('src/domain-cache/'),
    forbids: [/entityStoreAdapter/, /(?:^|\/)runtime\/replica\//],
    remedy:
      'the domain-cache is request/response glue (D116): live state flows through ' +
      'the reactive store, not through the entity-store adapter.',
  },
]

function visit(dir: string, files: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    const stat = statSync(path)
    if (stat.isDirectory()) {
      visit(path, files)
    } else if (/\.(ts|tsx)$/.test(entry)) {
      files.push(path)
    }
  }
  return files
}

/** Every module specifier this source imports/exports-from, with its line. */
function importSpecifiers(source: string): { line: number; spec: string }[] {
  const out: { line: number; spec: string }[] = []
  const pattern =
    /(?:import|export)\s+(?:type\s+)?[\s\S]*?from\s*['"]([^'"]+)['"]|import\s*\(\s*['"]([^'"]+)['"]\s*\)/g
  for (const match of source.matchAll(pattern)) {
    const spec = match[1] ?? match[2]
    if (!spec) continue
    const line = source.slice(0, match.index ?? 0).split('\n').length
    out.push({ line, spec })
  }
  return out
}

let failed = false

for (const file of visit(sourceRoot)) {
  const rel = relative(root, file).replaceAll('\\', '/')
  for (const rule of RULES) {
    if (!rule.appliesTo(rel)) continue
    if (rule.exempt?.has(rel)) continue
    for (const { line, spec } of importSpecifiers(readFileSync(file, 'utf8'))) {
      if (rule.forbids.some((pattern) => pattern.test(spec))) {
        failed = true
        console.error(
          `${rel}:${line}: [${rule.name}] forbidden import '${spec}'. ${rule.remedy}`,
        )
      }
    }
  }
}

if (failed) {
  process.exit(1)
}
