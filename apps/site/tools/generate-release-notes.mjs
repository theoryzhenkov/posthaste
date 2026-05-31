#!/usr/bin/env node
/**
 * Generate / refresh a release entry in the site content collection from a
 * GitHub release.
 *
 * Source of truth for the releases page is `src/content/releases/<version>.md`:
 * frontmatter carries the structured download data (version, tag, date,
 * per-platform assets); the Markdown body holds hand-authored dev notes.
 *
 * This script writes the frontmatter from a GitHub release object but PRESERVES
 * any existing body, so re-running it (e.g. when a release is edited, or to
 * backfill) never clobbers notes a human wrote.
 *
 * Usage:
 *   # one release, fetched from the API by tag
 *   node tools/generate-release-notes.mjs --tag v0.1.0-dogfood.39
 *   # backfill every published release
 *   node tools/generate-release-notes.mjs --all
 *   # from a release JSON object on stdin (e.g. the Actions event payload)
 *   gh api /repos/$REPO/releases/tags/$TAG | node tools/generate-release-notes.mjs --stdin
 *
 * GITHUB_TOKEN (optional) lifts the unauthenticated API rate limit.
 */
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import matter from 'gray-matter'

const REPO = process.env.RELEASES_REPO || 'theoryzhenkov/posthaste'
const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url))
const RELEASES_DIR = resolve(SCRIPT_DIR, '..', 'src', 'content', 'releases')

/**
 * Map a release tag to the user-facing app version, mirroring the desktop
 * version mapping in `.github/workflows/release.yml`:
 *   v0.1.0-dogfood.39 -> 0.1.39   (dogfood serial becomes the patch)
 *   v1.2.3            -> 1.2.3
 */
function versionFromTag(tag) {
  const bare = tag.replace(/^v/, '')
  const dogfood = bare.match(/^(\d+)\.(\d+)\.\d+-dogfood\.(\d+)$/)
  if (dogfood) return `${dogfood[1]}.${dogfood[2]}.${dogfood[3]}`
  return bare
}

/**
 * Classify a release asset into a download platform, or null to drop it from
 * the primary download grid (signatures, checksums, legacy variants, server).
 */
function classifyAsset(name) {
  if (/PosthasteDevTools/i.test(name)) return null // retired dual-build variant
  if (/\.(asc|sigstore\.json)$/i.test(name)) return null // detached signatures
  if (/^SHA256SUMS/i.test(name)) return null
  if (/release-gpg-public\.asc/i.test(name)) return null
  if (/^MACOS-INSTALL/i.test(name)) return null
  if (/^posthaste-serve-/i.test(name)) return null // self-host server bundle

  if (/_aarch64\.dmg$/i.test(name))
    return { os: 'macOS', arch: 'Apple Silicon', kind: 'dmg' }
  if (/_x64_.*\.msi$/i.test(name))
    return { os: 'Windows', arch: 'x64', kind: 'msi' }
  if (/_x64-setup\.exe$/i.test(name))
    return { os: 'Windows', arch: 'x64', kind: 'exe' }
  if (/_amd64\.AppImage$/i.test(name))
    return { os: 'Linux', arch: 'x86_64', kind: 'AppImage' }
  if (/_amd64\.deb$/i.test(name))
    return { os: 'Linux', arch: 'x86_64', kind: 'deb' }
  if (/\.x86_64\.rpm$/i.test(name))
    return { os: 'Linux', arch: 'x86_64', kind: 'rpm' }
  return null
}

function findAssetUrl(assets, predicate) {
  const hit = assets.find((a) => predicate(a.name))
  return hit ? hit.browser_download_url : undefined
}

async function api(path) {
  const headers = { Accept: 'application/vnd.github+json' }
  if (process.env.GITHUB_TOKEN) {
    headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`
  }
  const res = await fetch(`https://api.github.com${path}`, { headers })
  if (!res.ok) {
    throw new Error(`GitHub API ${path} -> ${res.status} ${res.statusText}`)
  }
  return res.json()
}

/** Build the frontmatter object for a release, dropping undefined keys. */
function frontmatterFor(release) {
  const version = versionFromTag(release.tag_name)
  const assets = (release.assets || [])
    .map((a) => {
      const klass = classifyAsset(a.name)
      if (!klass) return null
      return {
        ...klass,
        name: a.name,
        url: a.browser_download_url,
        size: a.size,
      }
    })
    .filter(Boolean)

  const order = { macOS: 0, Windows: 1, Linux: 2 }
  assets.sort(
    (a, b) => order[a.os] - order[b.os] || a.kind.localeCompare(b.kind),
  )

  const data = {
    version,
    tag: release.tag_name,
    date: (release.published_at || release.created_at || '').slice(0, 10),
    prerelease: Boolean(release.prerelease),
    assets,
  }
  const sha256 = findAssetUrl(release.assets || [], (n) =>
    /^SHA256SUMS$/i.test(n),
  )
  const gpgKey = findAssetUrl(release.assets || [], (n) =>
    /release-gpg-public\.asc/i.test(n),
  )
  if (sha256) data.sha256sums = sha256
  if (gpgKey) data.gpgKey = gpgKey
  return { data, version }
}

function writeRelease(release) {
  mkdirSync(RELEASES_DIR, { recursive: true })
  const { data, version } = frontmatterFor(release)
  const file = join(RELEASES_DIR, `${version}.md`)

  // Preserve any hand-authored notes body; only the frontmatter is regenerated.
  let body = ''
  if (existsSync(file)) {
    body = matter(readFileSync(file, 'utf8')).content
  }
  if (body.trim() === '') {
    body = `\n<!-- Dev notes for ${version} go here. Plain Markdown. -->\n`
  }

  writeFileSync(file, matter.stringify(body, data))
  const count = data.assets.length
  console.log(`wrote ${file} (${count} download${count === 1 ? '' : 's'})`)
}

async function main() {
  const args = process.argv.slice(2)
  if (args.includes('--stdin')) {
    const release = JSON.parse(readFileSync(0, 'utf8'))
    writeRelease(release)
    return
  }
  if (args.includes('--all')) {
    const releases = await api(`/repos/${REPO}/releases?per_page=100`)
    for (const release of releases) writeRelease(release)
    return
  }
  const tagIdx = args.indexOf('--tag')
  if (tagIdx !== -1 && args[tagIdx + 1]) {
    const release = await api(
      `/repos/${REPO}/releases/tags/${args[tagIdx + 1]}`,
    )
    writeRelease(release)
    return
  }
  console.error(
    'usage: generate-release-notes.mjs [--all | --tag <tag> | --stdin]',
  )
  process.exit(1)
}

main().catch((err) => {
  console.error(err.message)
  process.exit(1)
})
