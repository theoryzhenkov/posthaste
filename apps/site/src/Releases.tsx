import { Apple, Monitor, Terminal } from 'lucide-react'
import { useSyncExternalStore } from 'react'
import type {
  HomeContent,
  ReleaseAsset,
  ReleaseChannel,
  ReleaseEntry,
  ReleaseOs,
} from './content/types'
import { FooterSection, InstallHeader } from './SiteChrome'

const REPO = 'https://github.com/theoryzhenkov/posthaste'

const OS_ORDER: ReleaseOs[] = ['macOS', 'Windows', 'Linux']

const OS_ICON: Record<ReleaseOs, typeof Apple> = {
  macOS: Apple,
  Windows: Monitor,
  Linux: Terminal,
}

/** Human label for a desktop installer kind. */
const KIND_LABEL: Record<string, string> = {
  dmg: 'Disk image (.dmg)',
  exe: 'Installer (.exe)',
  msi: 'Installer (.msi)',
  AppImage: 'AppImage',
  deb: 'Debian / Ubuntu (.deb)',
  rpm: 'Fedora / RHEL (.rpm)',
}

const CHANNELS: { id: ReleaseChannel; label: string }[] = [
  { id: 'stable', label: 'Stable' },
  { id: 'nightly', label: 'Nightly' },
]

function formatSize(bytes: number): string {
  if (!bytes) return ''
  const mb = bytes / 1_000_000
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.round(bytes / 1000)} KB`
}

function formatDate(iso: string): string {
  const date = new Date(`${iso}T00:00:00Z`)
  if (Number.isNaN(date.getTime())) return iso
  return date.toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    timeZone: 'UTC',
  })
}

/** Short, button-friendly label for a desktop installer. */
function assetLabel(asset: ReleaseAsset): string {
  return KIND_LABEL[asset.kind] ?? asset.kind
}

function tagUrl(tag: string): string {
  return `${REPO}/releases/tag/${tag}`
}

function groupByOs(assets: ReleaseAsset[]): [ReleaseOs, ReleaseAsset[]][] {
  return OS_ORDER.map((os): [ReleaseOs, ReleaseAsset[]] => [
    os,
    assets.filter((asset) => asset.os === os),
  ]).filter(([, list]) => list.length > 0)
}

/** Best-effort OS detection for highlighting the visitor's platform. */
function detectOs(): ReleaseOs | null {
  if (typeof navigator === 'undefined') return null
  const ua = navigator.userAgent.toLowerCase()
  if (ua.includes('mac')) return 'macOS'
  if (ua.includes('win')) return 'Windows'
  if (ua.includes('linux') || ua.includes('x11')) return 'Linux'
  return null
}

const noopSubscribe = () => () => {}

/**
 * Resolve the visitor's OS only after hydration: the server snapshot is null
 * (so SSR markup highlights nothing and matches the first client render), then
 * the client snapshot detects the platform.
 */
function useDetectedOs(): ReleaseOs | null {
  return useSyncExternalStore(
    noopSubscribe,
    () => detectOs(),
    () => null,
  )
}

const CHANNEL_EVENT = 'posthaste:channelchange'

function channelSnapshot(): ReleaseChannel {
  if (typeof window === 'undefined') return 'stable'
  return new URLSearchParams(window.location.search).get('channel') ===
    'nightly'
    ? 'nightly'
    : 'stable'
}

function subscribeChannel(onChange: () => void): () => void {
  window.addEventListener('popstate', onChange)
  window.addEventListener(CHANNEL_EVENT, onChange)
  return () => {
    window.removeEventListener('popstate', onChange)
    window.removeEventListener(CHANNEL_EVENT, onChange)
  }
}

/**
 * Selected release channel, held in the URL (`?channel=nightly`) so it's
 * shareable and survives reloads. The store reads from the URL — server and
 * first client render both resolve to stable, then the client adopts the URL.
 */
function useChannel(): [ReleaseChannel, (channel: ReleaseChannel) => void] {
  const channel = useSyncExternalStore<ReleaseChannel>(
    subscribeChannel,
    channelSnapshot,
    () => 'stable',
  )

  const select = (next: ReleaseChannel) => {
    const url = new URL(window.location.href)
    if (next === 'stable') url.searchParams.delete('channel')
    else url.searchParams.set('channel', next)
    window.history.replaceState({}, '', url)
    window.dispatchEvent(new Event(CHANNEL_EVENT))
  }

  return [channel, select]
}

export function Releases({
  releases,
  footer,
}: {
  releases: ReleaseEntry[]
  footer: HomeContent['footer']
}) {
  const detectedOs = useDetectedOs()
  const [channel, setChannel] = useChannel()

  const channelReleases = releases.filter((r) => r.channel === channel)
  const latest = channelReleases[0]
  const desktopAssets = latest
    ? latest.assets.filter((a) => a.product === 'desktop')
    : []

  return (
    <main className="site-shell">
      <InstallHeader active="releases" />

      <section className="releases-hero" aria-labelledby="releases-title">
        <h1 id="releases-title">
          Install Posthaste{latest ? ` ${latest.version}` : ''}
        </h1>

        <div
          className="channel-switch"
          role="tablist"
          aria-label="Release channel"
        >
          {CHANNELS.map((c) => (
            <button
              type="button"
              role="tab"
              aria-selected={channel === c.id}
              className={`channel-tab${channel === c.id ? ' active' : ''}`}
              key={c.id}
              onClick={() => setChannel(c.id)}
            >
              {c.label}
            </button>
          ))}
        </div>

        {desktopAssets.length > 0 ? (
          <div className="download-grid">
            {groupByOs(desktopAssets).map(([os, assets]) => {
              const Icon = OS_ICON[os]
              const recommended = detectedOs === os
              return (
                <article
                  className={`download-card${recommended ? ' is-recommended' : ''}`}
                  key={os}
                >
                  {recommended ? (
                    <span className="download-badge">Your platform</span>
                  ) : null}
                  <div className="download-card-head">
                    <Icon aria-hidden="true" />
                    <div>
                      <h2>{os}</h2>
                      <span className="download-arch">{assets[0]?.arch}</span>
                    </div>
                  </div>
                  <ul className="download-options">
                    {assets.map((asset) => (
                      <li key={asset.name}>
                        <a href={asset.url} download>
                          <span>{assetLabel(asset)}</span>
                          <span className="download-size">
                            {formatSize(asset.size)}
                          </span>
                        </a>
                      </li>
                    ))}
                  </ul>
                </article>
              )
            })}
          </div>
        ) : (
          <div className="releases-empty">
            {channel === 'stable' ? (
              <>
                <p>
                  No stable release yet. Stable builds will land here once the
                  first one ships.
                </p>
                <button
                  type="button"
                  className="releases-empty-link"
                  onClick={() => setChannel('nightly')}
                >
                  Get the latest nightly →
                </button>
              </>
            ) : (
              <>
                <p>No nightly build is available right now.</p>
                <a className="releases-empty-link" href={`${REPO}/releases`}>
                  Browse releases on GitHub →
                </a>
              </>
            )}
          </div>
        )}

        {latest?.sha256sums || latest?.gpgKey ? (
          <p className="releases-verify">
            Verify your download:{' '}
            {latest.sha256sums ? (
              <a href={latest.sha256sums}>SHA-256 checksums</a>
            ) : null}
            {latest.sha256sums && latest.gpgKey ? ' · ' : null}
            {latest.gpgKey ? <a href={latest.gpgKey}>GPG signing key</a> : null}
          </p>
        ) : null}
      </section>

      {channelReleases.length > 0 ? (
        <section className="changelog" aria-labelledby="changelog-title">
          <h2 id="changelog-title">Changelog</h2>
          <ol className="changelog-list">
            {channelReleases.map((release) => (
              <li className="changelog-entry" key={release.version}>
                <div className="changelog-meta">
                  <h3>
                    {release.version}
                    {release.prerelease ? (
                      <span className="changelog-tag">nightly</span>
                    ) : null}
                  </h3>
                  <time dateTime={release.date}>
                    {formatDate(release.date)}
                  </time>
                </div>
                <div className="changelog-body">
                  {release.notesHtml ? (
                    <div
                      className="changelog-notes"
                      dangerouslySetInnerHTML={{ __html: release.notesHtml }}
                    />
                  ) : null}
                  <div className="changelog-downloads">
                    {release.assets
                      .filter((asset) => asset.product === 'desktop')
                      .map((asset) => (
                        <a
                          className="changelog-download"
                          href={asset.url}
                          download
                          key={asset.name}
                        >
                          {asset.os} · {assetLabel(asset)}
                        </a>
                      ))}
                    <a className="changelog-source" href={tagUrl(release.tag)}>
                      Release & all assets on GitHub →
                    </a>
                  </div>
                </div>
              </li>
            ))}
          </ol>
        </section>
      ) : null}

      <FooterSection content={footer} />
    </main>
  )
}
