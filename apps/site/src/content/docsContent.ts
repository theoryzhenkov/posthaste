import matter from 'gray-matter'
import { Marked } from 'marked'
import type { DocEntry } from './types'

/**
 * The user-facing guide, migrated from the main tree's `docs/` markdown into
 * the site so it matches posthaste.theor.net exactly (instead of living in a
 * separate docs tool). Each `docs/<slug>.md` carries frontmatter (title,
 * navLabel, description, order); the body is the source markdown unchanged, so
 * the originals stay the single source until phase 2 decides on a sync story.
 *
 * Rendering is deliberately isolated in its own `Marked` instance so the doc
 * link/heading rewriting below never leaks into the home/releases renderers.
 */
const docFiles = import.meta.glob<string>('./docs/*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
})

/**
 * mkdocs-material-compatible heading slug: lowercase, drop punctuation (em
 * dashes, colons, parens, inline-code backticks, `&amp;`), collapse whitespace
 * to single hyphens. Matches the `#anchor` targets the source docs link to.
 */
function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/<[^>]+>/g, '')
    .replace(/&#?\w+;/g, '')
    .replace(/[^a-z0-9\s-]/g, '')
    .trim()
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
}

/**
 * Rewrite a source cross-link to its /docs route. In-page anchors and external
 * URLs pass through untouched; relative `*.md` links (with optional `../` and
 * `#anchor`) map to `/docs/<stem>` (or `/docs` for the guide index). Links to
 * the still-in-mkdocs technical specs are only ever referenced as inline code
 * paths in the source, so nothing here points at the old docs tool.
 */
function rewriteHref(href: string): string {
  if (!href || href.startsWith('#')) return href
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return href
  const match = href.match(/^(?:\.\.?\/)*([\w.-]+?)\.md(#.*)?$/)
  if (!match) return href
  const [, stem, anchor = ''] = match
  return stem === 'index' ? `/docs${anchor}` : `/docs/${stem}${anchor}`
}

/** Inject stable, unique heading ids so in-page `#anchor` links resolve. */
function addHeadingIds(html: string): string {
  const seen = new Map<string, number>()
  return html.replace(
    /<h([1-6])>([\s\S]*?)<\/h\1>/g,
    (_match, level: string, inner: string) => {
      const base = slugify(inner)
      const count = seen.get(base) ?? 0
      seen.set(base, count + 1)
      const id = count === 0 ? base : `${base}-${count}`
      return `<h${level} id="${id}">${inner}</h${level}>`
    },
  )
}

function createRenderer(): Marked {
  const md = new Marked()
  md.use({
    walkTokens(token) {
      if (token.type === 'link') {
        token.href = rewriteHref(token.href)
      }
    },
  })
  return md
}

function requireString(
  data: Record<string, unknown>,
  key: string,
  file: string,
): string {
  const value = data[key]
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`${file} must define a non-empty "${key}" string`)
  }
  return value
}

let cached: DocEntry[] | null = null

export async function getDocs(): Promise<DocEntry[]> {
  if (cached) return cached

  const md = createRenderer()
  const entries = await Promise.all(
    Object.entries(docFiles).map(async ([file, raw]) => {
      const parsed = matter(raw)
      const data = parsed.data as Record<string, unknown>
      const slug = file.replace(/^\.\/docs\//, '').replace(/\.md$/, '')
      const order = typeof data.order === 'number' ? data.order : 999
      const title = requireString(data, 'title', file)
      const html = addHeadingIds(await md.parse(parsed.content.trim()))

      const entry: DocEntry = {
        slug,
        href: slug === 'index' ? '/docs' : `/docs/${slug}`,
        title,
        navLabel: typeof data.navLabel === 'string' ? data.navLabel : title,
        description: requireString(data, 'description', file),
        order,
        html,
      }
      return entry
    }),
  )

  entries.sort((a, b) => a.order - b.order || a.slug.localeCompare(b.slug))
  cached = entries
  return entries
}
