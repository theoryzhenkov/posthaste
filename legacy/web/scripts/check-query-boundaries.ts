import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const sourceRoot = join(root, 'src')

/**
 * The sidebar endpoint is a cross-account aggregate/bootstrap read, not a
 * domain authority. Only grandfathered composition/cache locations may read its
 * query key; feature code should read domain-named models instead.
 * Invalidation calls are tolerated during the transition because they do not
 * make the aggregate an authority.
 *
 * @spec docs/eph/DESIGN-L1-client-read-models#sidebar-boundary
 */
const allowedSidebarReadCounts = new Map<string, number>()

const sidebarQueryKeyPatterns = [
  /\bqueryKeys\.sidebar\b/,
  /\[\s*['"]sidebar['"]\s*\]/,
]

interface SidebarReadOccurrence {
  line: number
  text: string
}

let failed = false

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

function parenDelta(line: string): number {
  return [...line].reduce((total, char) => {
    if (char === '(') {
      return total + 1
    }
    if (char === ')') {
      return total - 1
    }
    return total
  }, 0)
}

function sidebarReadOccurrences(source: string): SidebarReadOccurrence[] {
  const occurrences: SidebarReadOccurrence[] = []
  let invalidateCallDepth = 0

  for (const [index, line] of source.split('\n').entries()) {
    const startsInvalidateCall = /\binvalidateQueries\s*\(/.test(line)
    if (startsInvalidateCall) {
      invalidateCallDepth += parenDelta(line)
      continue
    }

    if (invalidateCallDepth > 0) {
      invalidateCallDepth += parenDelta(line)
      continue
    }

    if (sidebarQueryKeyPatterns.some((pattern) => pattern.test(line))) {
      occurrences.push({ line: index + 1, text: line.trim() })
    }
  }

  return occurrences
}

for (const file of visit(sourceRoot)) {
  const rel = relative(root, file).replaceAll('\\', '/')
  const occurrences = sidebarReadOccurrences(readFileSync(file, 'utf8'))
  const allowedCount = allowedSidebarReadCounts.get(rel) ?? 0

  if (occurrences.length === allowedCount) {
    continue
  }

  failed = true
  if (occurrences.length > allowedCount) {
    for (const occurrence of occurrences.slice(allowedCount)) {
      console.error(
        `${rel}:${occurrence.line}: do not read queryKeys.sidebar outside the sidebar composition/cache boundary. ` +
          'Read domain query keys instead; aggregates should hydrate, not become authorities.',
      )
    }
    continue
  }

  console.error(
    `${rel}: expected ${allowedCount} grandfathered sidebar read(s), found ${occurrences.length}.`,
  )
}

if (failed) {
  process.exit(1)
}
