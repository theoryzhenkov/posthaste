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
 *   # reconcile the site with the live set of stable + nightly GitHub
 *   # releases, pruning entries whose release no longer exists (so the site
 *   # never links to 404s). Rolling channel pointers / RC builds are skipped.
 *   node tools/generate-release-notes.mjs --all
 *   # one release, fetched from the API by tag (stable or nightly)
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
import { fileURLToPath, pathToFileURL } from 'node:url'
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
 * Map a tag to its release channel, or null for tags we don't list: the
 * rolling channel pointers (tags `nightly`/`stable`), RC builds, and legacy
 * dogfood serials.
 *   v1.2.3              -> 'stable'
 *   v0.2.0-nightly.44   -> 'nightly'
 */
export function channelFromTag(tag) {
  if (/^v?\d+\.\d+\.\d+$/i.test(tag)) return 'stable'
  if (/^v?\d+\.\d+\.\d+-nightly\.\d+$/i.test(tag)) return 'nightly'
  return null
}

/** Normalise a platform token from an asset name to a release OS. */
function osFromToken(token) {
  const t = token.toLowerCase()
  if (t === 'darwin' || t === 'macos') return 'macOS'
  if (t === 'windows') return 'Windows'
  return 'Linux'
}

/**
 * Assets we deliberately never surface as downloads: detached signatures,
 * checksums, the GPG key, the Tauri updater manifest/bundle, and the web dist
 * (all still reachable via the GitHub release for the curious).
 */
export function isIgnoredAsset(name) {
  return (
    /\.(asc|sig|sigstore\.json)$/i.test(name) ||
    /^SHA256SUMS/i.test(name) ||
    /release-gpg-public\.asc/i.test(name) ||
    /^MACOS-INSTALL/i.test(name) ||
    /^latest\.json$/i.test(name) ||
    /\.app\.tar\.gz$/i.test(name) ||
    /PosthasteWeb/i.test(name) ||
    /PosthasteDevTools/i.test(name)
  )
}

/**
 * Classify a release asset into a downloadable product + platform, or null for
 * assets we don't surface. Returns { product, os, arch, kind }.
 */
export function classifyAsset(name) {
  if (isIgnoredAsset(name)) return null

  // Desktop app installers.
  if (/_aarch64\.dmg$/i.test(name))
    return {
      product: 'desktop',
      os: 'macOS',
      arch: 'Apple Silicon',
      kind: 'dmg',
    }
  if (/_x64(_.*)?\.msi$/i.test(name))
    return { product: 'desktop', os: 'Windows', arch: 'x64', kind: 'msi' }
  if (/_x64-setup\.exe$/i.test(name))
    return { product: 'desktop', os: 'Windows', arch: 'x64', kind: 'exe' }
  if (/_amd64\.AppImage$/i.test(name))
    return { product: 'desktop', os: 'Linux', arch: 'x86_64', kind: 'AppImage' }
  if (/_amd64\.deb$/i.test(name))
    return { product: 'desktop', os: 'Linux', arch: 'x86_64', kind: 'deb' }
  if (/\.x86_64\.rpm$/i.test(name))
    return { product: 'desktop', os: 'Linux', arch: 'x86_64', kind: 'rpm' }

  // posthastectl — the command-line client.
  const cli = name.match(/PosthasteCTL\w*-(darwin|linux|windows)-(arm64|x64)/i)
  if (cli) {
    return {
      product: 'cli',
      os: osFromToken(cli[1]),
      arch: cli[2].toLowerCase(),
      kind: 'binary',
    }
  }

  // posthaste daemon — the self-host runtime bundle.
  const daemon = name.match(/PosthasteDaemon\w*-(linux|macos|windows)/i)
  if (daemon) {
    const arch = (name.match(/(x86_64|aarch64|arm64)/i) || [])[1] || ''
    return {
      product: 'daemon',
      os: osFromToken(daemon[1]),
      arch,
      kind: 'tar.gz',
    }
  }
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

  // Surface artifact-naming drift: anything that matched no classifier and
  // isn't explicitly ignored would silently disappear from the download UI.
  const unclassified = (release.assets || [])
    .map((a) => a.name)
    .filter((name) => !classifyAsset(name) && !isIgnoredAsset(name))
  if (unclassified.length > 0) {
    console.warn(
      `WARN ${release.tag_name}: ${unclassified.length} asset(s) matched no ` +
        `classifier and aren't ignored — downloads may be missing: ` +
        unclassified.join(', '),
    )
  }
  if (!assets.some((a) => a.product === 'desktop')) {
    console.warn(
      `WARN ${release.tag_name}: no desktop installers classified — the ` +
        `install grid will be empty.`,
    )
  }

  const productOrder = { desktop: 0, cli: 1, daemon: 2 }
  const osOrder = { macOS: 0, Windows: 1, Linux: 2 }
  assets.sort(
    (a, b) =>
      productOrder[a.product] - productOrder[b.product] ||
      osOrder[a.os] - osOrder[b.os] ||
      a.kind.localeCompare(b.kind),
  )

  const data = {
    version,
    tag: release.tag_name,
    date: (release.published_at || release.created_at || '').slice(0, 10),
    channel: channelFromTag(release.tag_name),
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
    if (!channelFromTag(release.tag_name)) continue
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
    if (!channelFromTag(release.tag_name)) return
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

// Only run the CLI when invoked directly — importing for tests must not exec.
if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  main().catch((err) => {
    console.error(err.message)
    process.exit(1)
  })
}
