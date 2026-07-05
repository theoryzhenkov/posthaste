import { defineCollection } from 'astro:content'
import { glob } from 'astro/loaders'
import { docsSchema } from '@astrojs/starlight/schema'
import { routeIdFromRepoRel } from './docs-routing.mjs'

// Starlight's `docs` collection, sourced in place from two trees under the repo
// root: the user guide (`apps/site/src/content/guide`) and the canonical,
// @spec-referenced technical specs (`docs/**`, minus the internal `eph`/`issues`
// corpora). Reading the specs where they live — rather than copying them into
// the site — is what keeps the ~350 `@spec docs/...` code references valid. The
// route id for every file comes from the shared routing table so in-body links
// (rewritten by the rehype plugin) resolve to the same URLs.
const docs = defineCollection({
  loader: glob({
    base: '../../',
    pattern: [
      'docs/**/[^_]*.{md,mdx}',
      '!docs/eph/**',
      '!docs/issues/**',
      'apps/site/src/content/guide/**/[^_]*.{md,mdx}',
    ],
    generateId: ({ entry }) =>
      routeIdFromRepoRel(entry) ?? entry.replace(/\.mdx?$/, ''),
  }),
  schema: docsSchema(),
})

export const collections = { docs }
