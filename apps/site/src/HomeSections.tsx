import { Terminal } from 'lucide-react'
import type { CSSProperties } from 'react'
import type { HomeContent } from './content/types'
import { useLandscapeTime } from './hooks'

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

export function ScreenshotsSection() {
  return (
    <section className="screenshots" aria-labelledby="screenshots-title">
      <div className="section-header" data-reveal>
        <h2 id="screenshots-title">A look inside</h2>
      </div>
      <div className="screenshot-grid">
        {SCREENSHOTS.map((shot) => (
          <figure className="screenshot" data-reveal key={shot.src}>
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
  )
}

export function WizardSection() {
  return (
    <section
      className="wizard-teaser"
      aria-labelledby="wizard-teaser-title"
      data-reveal
    >
      <Terminal className="wizard-teaser-icon" aria-hidden="true" />
      <div className="wizard-teaser-body">
        <h2 id="wizard-teaser-title">Run Posthaste on your own server</h2>
        <p>
          Self-host the backend and join runtime nodes across machines —
          distributed, TLS-ready, and one command away.
        </p>
      </div>
      <a className="wizard-teaser-link" href="/wizard">
        Wizard guide →
      </a>
    </section>
  )
}

export function LandscapeValuesSection({
  content,
}: {
  content: HomeContent['openSource']
}) {
  const landscapeTime = useLandscapeTime()
  const landscapeStyle = {
    '--celestial-x': `${landscapeTime.celestialX}%`,
    '--celestial-y': `${landscapeTime.celestialY}%`,
  } as CSSProperties

  return (
    <section
      className="landscape-section"
      aria-labelledby="values-title"
      data-reveal
    >
      <div className="landscape-copy">
        <h2 id="values-title">{content.title}</h2>
        <div dangerouslySetInnerHTML={{ __html: content.html }} />
      </div>
      <div
        className={`landscape-canvas ${landscapeTime.phase}`}
        style={landscapeStyle}
        aria-hidden="true"
      >
        <span className="celestial" />
        <div className="landscape-track">
          <LandscapeSegment />
          <LandscapeSegment />
        </div>
        <img className="landscape-logo" src="/posthaste-logo.svg" alt="" />
      </div>
    </section>
  )
}

function LandscapeSegment() {
  return (
    <div className="landscape-segment">
      <svg
        className="landscape-terrain"
        viewBox="0 0 1200 260"
        preserveAspectRatio="none"
      >
        <path
          className="terrain-back"
          d="M0 148C48 132 90 130 138 146C188 164 228 167 282 150C334 134 382 120 438 142C498 166 538 181 604 160C672 138 716 121 782 146C846 170 894 183 958 158C1018 135 1066 124 1126 139C1156 147 1180 154 1200 148V260H0Z"
        />
        <path
          className="terrain-mid"
          d="M0 178C36 189 74 196 126 186C184 175 218 147 284 158C350 169 388 207 456 190C522 173 558 139 624 154C692 170 726 210 796 196C870 181 908 146 978 162C1042 177 1078 199 1136 188C1164 183 1186 174 1200 178V260H0Z"
        />
        <path
          className="terrain-front"
          d="M0 218C46 205 86 200 138 214C192 229 230 238 286 222C344 206 386 184 448 202C512 221 548 242 614 228C680 214 718 185 784 198C854 212 896 239 964 224C1028 210 1068 189 1130 203C1160 210 1184 222 1200 218V260H0Z"
        />
      </svg>
    </div>
  )
}
