import type { CSSProperties } from 'react'
import type { HomeContent } from './content/types'
import { useLandscapeTime } from './hooks'

const palette = ['blue', 'coral', 'sage', 'amber', 'violet']

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

export function NotesSection({ content }: { content: HomeContent }) {
  return (
    <section className="notes-section" id="notes" aria-labelledby="notes-title">
      <div className="section-header" data-reveal>
        <h2 id="notes-title">{content.notesHeading.title}</h2>
      </div>
      <div className="note-list">
        {content.notes.map((note) => (
          <article className="note-row" data-reveal key={note.label}>
            <span>{note.label}</span>
            <h3>{note.title}</h3>
            <div dangerouslySetInnerHTML={{ __html: note.html }} />
          </article>
        ))}
      </div>
    </section>
  )
}

export function ThemeSection({ content }: { content: HomeContent['theme'] }) {
  return (
    <section
      className="theme-section"
      id="themes"
      aria-labelledby="themes-title"
    >
      <div className="theme-copy" data-reveal>
        <h2 id="themes-title">{content.title}</h2>
        <div dangerouslySetInnerHTML={{ __html: content.html }} />
      </div>
      <div className="glass-panel" data-reveal>
        <div className="glass-title">Theme preview</div>
        <div className="swatch-row" aria-hidden="true">
          {palette.map((color) => (
            <span className={color} key={color} />
          ))}
        </div>
        <div className="glass-lines" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
      </div>
    </section>
  )
}
