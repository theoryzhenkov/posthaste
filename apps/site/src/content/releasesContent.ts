import matter from 'gray-matter'
import { marked } from 'marked'
import type { ReleaseAsset, ReleaseEntry, ReleaseOs } from './types'

/**
 * Release entries are the source of truth for the downloads/changelog page.
 * Each `releases/<version>.md` is generated from a GitHub release by
 * `tools/generate-release-notes.mjs` (frontmatter) and optionally annotated
 * with a hand-authored Markdown body (dev notes). New files dropped in by CI
 * are picked up here automatically — no loader edits needed.
 */
const releaseFiles = import.meta.glob<string>('./releases/*.md', {
  query: '?raw',
  import: 'default',
  eager: true,
})

const VALID_OS: readonly ReleaseOs[] = ['macOS', 'Windows', 'Linux']

function asString(value: unknown, key: string, file: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new Error(`${file} must define a non-empty "${key}" string`)
  }
  return value
}

function parseAssets(value: unknown, file: string): ReleaseAsset[] {
  if (value == null) return []
  if (!Array.isArray(value)) {
    throw new Error(`${file} "assets" must be a list`)
  }
  return value.map((raw, index) => {
    const entry = raw as Record<string, unknown>
    const os = asString(entry.os, `assets[${index}].os`, file) as ReleaseOs
    if (!VALID_OS.includes(os)) {
      throw new Error(
        `${file} assets[${index}].os "${os}" is not a known platform`,
      )
    }
    return {
      os,
      arch: asString(entry.arch, `assets[${index}].arch`, file),
      kind: asString(entry.kind, `assets[${index}].kind`, file),
      name: asString(entry.name, `assets[${index}].name`, file),
      url: asString(entry.url, `assets[${index}].url`, file),
      size: typeof entry.size === 'number' ? entry.size : 0,
    }
  })
}

/** Descending version order: newest release first. */
function compareVersionsDesc(a: string, b: string): number {
  const pa = a.split('.').map(Number)
  const pb = b.split('.').map(Number)
  const len = Math.max(pa.length, pb.length)
  for (let i = 0; i < len; i += 1) {
    const diff = (pb[i] ?? 0) - (pa[i] ?? 0)
    if (diff !== 0) return diff
  }
  return 0
}

let cached: ReleaseEntry[] | null = null

export async function getReleases(): Promise<ReleaseEntry[]> {
  if (cached) return cached

  const entries = await Promise.all(
    Object.entries(releaseFiles).map(async ([file, raw]) => {
      const parsed = matter(raw)
      const data = parsed.data as Record<string, unknown>
      const notesHtml = await marked.parse(parsed.content.trim())

      const entry: ReleaseEntry = {
        version: asString(data.version, 'version', file),
        tag: asString(data.tag, 'tag', file),
        date: asString(data.date, 'date', file),
        prerelease: data.prerelease === true,
        assets: parseAssets(data.assets, file),
        notesHtml,
      }
      if (typeof data.sha256sums === 'string')
        entry.sha256sums = data.sha256sums
      if (typeof data.gpgKey === 'string') entry.gpgKey = data.gpgKey
      return entry
    }),
  )

  entries.sort((a, b) => compareVersionsDesc(a.version, b.version))
  cached = entries
  return entries
}
