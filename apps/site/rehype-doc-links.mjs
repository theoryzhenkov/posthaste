// Rehype plugin: rewrite in-body markdown cross-links so the canonical `docs/`
// sources can stay byte-identical while rendering under Starlight's /docs
// routes. Relative `*.md` links are resolved against the source file and mapped
// through the shared routing table; links into the internal `eph/`/`issues/`
// corpus become GitHub blob URLs (external, but not dead). Absolute URLs,
// in-page anchors, and non-`.md` links pass through untouched.

import { dirname, relative, resolve } from 'node:path'
import {
  GITHUB_BLOB,
  REPO_ROOT,
  routeIdFromRepoRel,
} from './src/docs-routing.mjs'

function toPosix(p) {
  return p.split('\\').join('/')
}

/** Walk a hast tree, invoking `fn` on every element node. */
function walk(node, fn) {
  if (!node || typeof node !== 'object') return
  if (node.type === 'element') fn(node)
  const children = node.children
  if (Array.isArray(children)) {
    for (const child of children) walk(child, fn)
  }
}

export default function rehypeDocLinks() {
  return (tree, file) => {
    const sourcePath = file?.path ?? file?.history?.[0]
    if (!sourcePath) return
    const sourceDir = dirname(sourcePath)

    walk(tree, (node) => {
      if (node.tagName !== 'a') return
      const href = node.properties?.href
      if (typeof href !== 'string' || href.length === 0) return
      if (href.startsWith('#') || href.startsWith('/')) return
      if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return // has a scheme (http:, mailto:)

      const match = href.match(/^([^#?]*\.mdx?)([#?].*)?$/i)
      if (!match) return
      const [, target, suffix = ''] = match

      const absTarget = resolve(sourceDir, target)
      const repoRel = toPosix(relative(REPO_ROOT, absTarget))

      // Internal-only corpus: point at the GitHub source rather than 404.
      if (
        repoRel.startsWith('docs/eph/') ||
        repoRel.startsWith('docs/issues/')
      ) {
        node.properties.href = `${GITHUB_BLOB}${repoRel}${suffix}`
        return
      }

      const id = routeIdFromRepoRel(repoRel)
      if (!id) return
      node.properties.href = `/${id}/${suffix}`
    })
  }
}
