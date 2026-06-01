import { Download, Grip, Pin } from 'lucide-react'
import type { HomeContent } from './content/types'

/**
 * Site-wide chrome shared across pages (home, releases). Links are
 * root-absolute so they resolve from any route.
 */
export function InstallHeader({ active }: { active?: 'releases' } = {}) {
  return (
    <header className="install-header" aria-label="Install and navigation">
      <div className="install-header-grip" aria-hidden="true">
        <Grip />
      </div>
      <button
        type="button"
        className="install-header-pin is-pinned"
        aria-label="Pinned"
        aria-pressed="true"
      >
        <Pin aria-hidden="true" />
      </button>
      <a className="install-header-button" href="/releases">
        <Download aria-hidden="true" />
        <span>Try beta</span>
      </a>
      <nav className="install-header-nav" aria-label="Site">
        <a href="/#notes">Builders</a>
        <a href="/#themes">Interface</a>
        <a
          href="/releases"
          aria-current={active === 'releases' ? 'page' : undefined}
        >
          Releases
        </a>
      </nav>
    </header>
  )
}

export function FooterSection({ content }: { content: HomeContent['footer'] }) {
  return (
    <footer className="footer-section">
      <span>{content.brand}</span>
      <div dangerouslySetInnerHTML={{ __html: content.html }} />
    </footer>
  )
}
