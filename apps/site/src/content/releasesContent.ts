import matter from 'gray-matter'
import { marked } from 'marked'
import { compareReleasesDesc } from './releaseOrdering'
import type {
  ReleaseAsset,
  ReleaseChannel,
  ReleaseEntry,
  ReleaseOs,
  ReleaseProduct,
} from './types'

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
const VALID_PRODUCT: readonly ReleaseProduct[] = ['desktop', 'cli', 'daemon']
const VALID_CHANNEL: readonly ReleaseChannel[] = ['stable', 'nightly']

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
    const product = asString(
      entry.product,
      `assets[${index}].product`,
      file,
    ) as ReleaseProduct
    if (!VALID_PRODUCT.includes(product)) {
      throw new Error(
        `${file} assets[${index}].product "${product}" is not a known product`,
      )
    }
    return {
      product,
      os,
      // arch is display-only and optional (e.g. the universal macOS daemon).
      arch: typeof entry.arch === 'string' ? entry.arch : '',
      kind: asString(entry.kind, `assets[${index}].kind`, file),
      name: asString(entry.name, `assets[${index}].name`, file),
      url: asString(entry.url, `assets[${index}].url`, file),
      size: typeof entry.size === 'number' ? entry.size : 0,
    }
  })
}

let cached: ReleaseEntry[] | null = null

export async function getReleases(): Promise<ReleaseEntry[]> {
  if (cached) return cached

  const entries = await Promise.all(
    Object.entries(releaseFiles).map(async ([file, raw]) => {
      const parsed = matter(raw)
      const data = parsed.data as Record<string, unknown>
      const notesHtml = await marked.parse(parsed.content.trim())

      const channel = asString(data.channel, 'channel', file) as ReleaseChannel
      if (!VALID_CHANNEL.includes(channel)) {
        throw new Error(`${file} channel "${channel}" is not a known channel`)
      }
      const entry: ReleaseEntry = {
        version: asString(data.version, 'version', file),
        tag: asString(data.tag, 'tag', file),
        date: asString(data.date, 'date', file),
        channel,
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

  entries.sort(compareReleasesDesc)
  cached = entries
  return entries
}
