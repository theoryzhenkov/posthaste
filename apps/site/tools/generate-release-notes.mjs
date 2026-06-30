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
 *   # reconcile the site with the live set of *stable* GitHub releases,
 *   # pruning entries whose release no longer exists (so the site never links
 *   # to 404s). Nightly/dogfood/RC builds are never listed.
 *   node tools/generate-release-notes.mjs --all
 *   # one release, fetched from the API by tag (stable only)
 *   node tools/generate-release-notes.mjs --tag v1.2.3
 *   # prune the entry for one deleted tag
 *   node tools/generate-release-notes.mjs --delete v1.2.3
 *   # from a release JSON object on stdin (e.g. the Actions event payload)
 *   gh api /repos/$REPO/releases/tags/$TAG | node tools/generate-release-notes.mjs --stdin
 *
 * GITHUB_TOKEN (optional) lifts the unauthenticated API rate limit.
 */
import {
  readFileSync,
  writeFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  unlinkSync,
} from 'node:fs'
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
 * Only stable releases get a website entry — a plain semver tag with no
 * prerelease suffix (e.g. v0.2.0, v1.2.3). Nightly/dogfood/RC builds and the
 * rolling channel releases (tags `nightly`/`stable`) are deliberately
 * excluded: they're ephemeral and get pruned from GitHub, which would leave the
 * downloads page pointing at 404s.
 */
function isStableTag(tag) {
  return /^v?\d+\.\d+\.\d+$/i.test(tag)
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

/** Fetch every published release, paging until exhausted. */
async function fetchAllReleases() {
  const all = []
  for (let page = 1; ; page += 1) {
    const batch = await api(`/repos/${REPO}/releases?per_page=100&page=${page}`)
    all.push(...batch)
    if (batch.length < 100) break
  }
  return all
}

/** Delete any entry whose version is no longer in the live release set. */
function pruneStale(liveVersions) {
  if (!existsSync(RELEASES_DIR)) return
  const stale = readdirSync(RELEASES_DIR)
    .filter((f) => f.endsWith('.md'))
    .filter((f) => !liveVersions.has(f.replace(/\.md$/, '')))
  for (const file of stale) {
    unlinkSync(join(RELEASES_DIR, file))
    console.log(`pruned ${file} (release no longer published)`)
  }
}

/** Reconcile the whole collection: write every live stable release, then
 *  prune entries whose release has been deleted from GitHub. */
async function reconcileAll() {
  const releases = await fetchAllReleases()
  const liveVersions = new Set()
  for (const release of releases) {
    if (!isStableTag(release.tag_name)) continue
    writeRelease(release)
    liveVersions.add(versionFromTag(release.tag_name))
  }
  pruneStale(liveVersions)
}

function deleteRelease(tag) {
  const version = versionFromTag(tag)
  const file = join(RELEASES_DIR, `${version}.md`)
  if (existsSync(file)) {
    unlinkSync(file)
    console.log(`pruned ${version}.md (release deleted)`)
  } else {
    console.log(`no entry to prune for ${tag}`)
  }
}

async function main() {
  const args = process.argv.slice(2)
  if (args.includes('--stdin')) {
    const release = JSON.parse(readFileSync(0, 'utf8'))
    if (!isStableTag(release.tag_name)) return
    writeRelease(release)
    return
  }
  if (args.includes('--all')) {
    await reconcileAll()
    return
  }
  const deleteIdx = args.indexOf('--delete')
  if (deleteIdx !== -1 && args[deleteIdx + 1]) {
    deleteRelease(args[deleteIdx + 1])
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
    'usage: generate-release-notes.mjs [--all | --tag <tag> | --delete <tag> | --stdin]',
  )
  process.exit(1)
}

main().catch((err) => {
  console.error(err.message)
  process.exit(1)
})
