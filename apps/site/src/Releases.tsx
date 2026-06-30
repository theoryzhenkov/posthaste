import { Apple, Monitor, Terminal } from 'lucide-react'
import { useSyncExternalStore } from 'react'
import type {
  HomeContent,
  ReleaseAsset,
  ReleaseEntry,
  ReleaseOs,
} from './content/types'
import { FooterSection, InstallHeader } from './SiteChrome'

/** App screenshots for the gallery, captured from a seeded demo account. */
const SCREENSHOTS = [
  {
    src: '/screenshots/conversations.png',
    caption: 'Threaded conversation view',
    alt: 'Posthaste inbox showing a threaded conversation with nested replies',
  },
  {
    src: '/screenshots/reader.png',
    caption: 'Reading a thread',
    alt: 'Posthaste reading pane with an open email thread',
  },
  {
    src: '/screenshots/compose.png',
    caption: 'Compose',
    alt: 'Posthaste compose window with a draft reply',
  },
  {
    src: '/screenshots/command-palette.png',
    caption: 'Command palette',
    alt: 'Posthaste command palette open over the inbox',
  },
]

const OS_ORDER: ReleaseOs[] = ['macOS', 'Windows', 'Linux']

const OS_ICON: Record<ReleaseOs, typeof Apple> = {
  macOS: Apple,
  Windows: Monitor,
  Linux: Terminal,
}

/** Human label for an installer kind. */
const KIND_LABEL: Record<string, string> = {
  dmg: 'Disk image (.dmg)',
  exe: 'Installer (.exe)',
  msi: 'Installer (.msi)',
  AppImage: 'AppImage',
  deb: 'Debian / Ubuntu (.deb)',
  rpm: 'Fedora / RHEL (.rpm)',
}

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
 * (so SSR markup highlights nothing and matches first client render), then the
 * client snapshot detects the platform.
 */
function useDetectedOs(): ReleaseOs | null {
  return useSyncExternalStore(
    noopSubscribe,
    () => detectOs(),
    () => null,
  )
}

export function Releases({
  releases,
  footer,
}: {
  releases: ReleaseEntry[]
  footer: HomeContent['footer']
}) {
  const detectedOs = useDetectedOs()
  const latest = releases[0]

  return (
    <main className="site-shell">
      <InstallHeader active="releases" />

      <section className="releases-hero" aria-labelledby="releases-title">
        <h1 id="releases-title">
          Try Posthaste{latest ? ` ${latest.version}` : ''}
        </h1>
        {latest ? (
          <p className="releases-subtitle">
            Released {formatDate(latest.date)} ·{' '}
            <a
              href={`https://github.com/theoryzhenkov/posthaste/releases/tag/${latest.tag}`}
            >
              {latest.tag}
            </a>
          </p>
        ) : null}

        {latest ? null : (
          <div className="releases-empty">
            <p>
              No stable release yet. Beta builds for macOS, Windows, and Linux
              ship on GitHub while we get there.
            </p>
            <a
              className="releases-empty-link"
              href="https://github.com/theoryzhenkov/posthaste/releases"
            >
              Beta builds on GitHub →
            </a>
          </div>
        )}

        {latest ? (
          <div className="download-grid">
            {groupByOs(latest.assets).map(([os, assets]) => {
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
                          <span>{KIND_LABEL[asset.kind] ?? asset.kind}</span>
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
        ) : null}

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

      <section className="screenshots" aria-labelledby="screenshots-title">
        <h2 id="screenshots-title">A look inside</h2>
        <div className="screenshot-grid">
          {SCREENSHOTS.map((shot) => (
            <figure className="screenshot" key={shot.src}>
              <img
                src={shot.src}
                alt={shot.alt}
                width={1440}
                height={900}
                loading="lazy"
                decoding="async"
              />
              <figcaption>{shot.caption}</figcaption>
            </figure>
          ))}
        </div>
      </section>

      {releases.length > 0 ? (
        <section className="changelog" aria-labelledby="changelog-title">
          <h2 id="changelog-title">Changelog</h2>
          <ol className="changelog-list">
            {releases.map((release) => (
              <li className="changelog-entry" key={release.version}>
                <div className="changelog-meta">
                  <h3>
                    {release.version}
                    {release.prerelease ? (
                      <span className="changelog-tag">pre-release</span>
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
                    {release.assets.map((asset) => (
                      <a
                        className="changelog-download"
                        href={asset.url}
                        download
                        key={asset.name}
                      >
                        {asset.os} {asset.kind}
                      </a>
                    ))}
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
