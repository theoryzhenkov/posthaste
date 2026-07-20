// Shared docs routing map. Used by BOTH the content-collection loader
// (`content.config.ts`, to assign each source file its Starlight route id) and
// the markdown rehype plugin (`rehype-doc-links.mjs`, to rewrite in-body
// cross-links to those same routes). Keeping the mapping in one module is what
// lets Starlight read the canonical `docs/` tree in place while its links still
// resolve.
//
// The docs corpus is served from TWO source trees, both under the repo root:
//   - the user GUIDE at `apps/site/src/content/guide/*.md`  → /docs, /docs/<name>
//   - the technical SPECS at `docs/**/*.md` (canonical, @spec-referenced)
//                                                            → /docs/<path>
// `docs/index.md` (the specs landing) owns /docs (the user guide that used to
// own it has been removed for hand-rewrite). `docs/eph/**` and `docs/issues/**`
// stay internal.

import { fileURLToPath } from 'node:url'
import { relative } from 'node:path'

/** Absolute path to the repository root (three levels up from this file). */
export const REPO_ROOT = fileURLToPath(new URL('../../../', import.meta.url))

const GUIDE_PREFIX = 'apps/site/src/content/guide/'

/** GitHub blob base for links into the internal (unpublished) corpus. */
export const GITHUB_BLOB =
  'https://github.com/theoryzhenkov/posthaste/blob/main/'

/**
 * Map a repo-root-relative POSIX path (with or without extension) to its
 * Starlight route id, or `null` when the file is not a published doc.
 */
export function routeIdFromRepoRel(relPosix) {
  const p = relPosix.replace(/\\/g, '/').replace(/\.mdx?$/, '')

  if (p.startsWith(GUIDE_PREFIX)) {
    const name = p.slice(GUIDE_PREFIX.length)
    return name === 'index' ? 'docs' : `docs/${name}`
  }

  if (p === 'docs' || p === 'docs/index') return 'docs'

  if (p.startsWith('docs/')) {
    if (p.startsWith('docs/eph/') || p.startsWith('docs/issues/')) return null
    return p
  }

  return null
}

/** Convenience wrapper for absolute source paths. */
export function routeIdFromAbsPath(absPath) {
  return routeIdFromRepoRel(relative(REPO_ROOT, absPath).split('\\').join('/'))
}
