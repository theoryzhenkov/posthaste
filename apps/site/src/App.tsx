import {
  Archive,
  Download,
  Flag,
  Grip,
  Mail,
  Palette,
  Pin,
  Search,
  Settings,
} from 'lucide-react'
import { type CSSProperties, useEffect, useState } from 'react'
import type { HomeContent, SiteMessage } from './content/types'

interface Mailbox {
  label: string
  count: string
  color: string
  active?: boolean
}

type LandscapePhase = 'night' | 'morning' | 'day' | 'evening'

interface LandscapeTimeState {
  phase: LandscapePhase
  celestialX: number
  celestialY: number
}

const initialLandscapeTimeState: LandscapeTimeState = {
  phase: 'day',
  celestialX: 41,
  celestialY: 22,
}

const mailboxes: Mailbox[] = [
  { label: 'All Inboxes', count: '42', color: 'coral', active: true },
  { label: 'VIP', count: '8', color: 'blue' },
  { label: 'Bills', count: '5', color: 'violet' },
  { label: 'Read Later', count: '16', color: 'amber' },
  { label: 'Newsletters', count: '31', color: 'sage' },
]

const palette = ['blue', 'coral', 'sage', 'amber', 'violet']

function getLandscapeTimeState(date = new Date()): LandscapeTimeState {
  const minutes = date.getHours() * 60 + date.getMinutes()
  const sunrise = 6 * 60
  const morningEnd = 10 * 60
  const eveningStart = 17 * 60
  const sunset = 20 * 60

  let phase: LandscapePhase = 'night'

  if (minutes >= sunrise && minutes < morningEnd) {
    phase = 'morning'
  } else if (minutes >= morningEnd && minutes < eveningStart) {
    phase = 'day'
  } else if (minutes >= eveningStart && minutes < sunset) {
    phase = 'evening'
  }

  if (phase === 'night') {
    const nightStart = sunset
    const nightLength = 10 * 60
    const nightMinutes =
      minutes >= nightStart
        ? minutes - nightStart
        : minutes + 24 * 60 - nightStart
    const progress = nightMinutes / nightLength

    return {
      phase,
      celestialX: 8 + progress * 84,
      celestialY: 66 - Math.sin(progress * Math.PI) * 34,
    }
  }

  const dayProgress = Math.max(
    0,
    Math.min(1, (minutes - sunrise) / (sunset - sunrise)),
  )

  return {
    phase,
    celestialX: 8 + dayProgress * 66,
    celestialY: 72 - Math.sin(dayProgress * Math.PI) * 50,
  }
}

function useLandscapeTime() {
  const [timeState, setTimeState] = useState(initialLandscapeTimeState)

  useEffect(() => {
    const initial = window.setTimeout(() => {
      setTimeState(getLandscapeTimeState())
    }, 0)
    const interval = window.setInterval(() => {
      setTimeState(getLandscapeTimeState())
    }, 60_000)

    return () => {
      window.clearTimeout(initial)
      window.clearInterval(interval)
    }
  }, [])

  return timeState
}

function useReveal() {
  useEffect(() => {
    const reduceMotion = window.matchMedia(
      '(prefers-reduced-motion: reduce)',
    ).matches

    if (reduceMotion) {
      document
        .querySelectorAll<HTMLElement>('[data-reveal]')
        .forEach((element) => element.classList.add('is-visible'))
      return
    }

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add('is-visible')
          }
        })
      },
      { rootMargin: '0px 0px -10% 0px', threshold: 0.18 },
    )

    document
      .querySelectorAll<HTMLElement>('[data-reveal]')
      .forEach((element) => observer.observe(element))

    return () => observer.disconnect()
  }, [])
}

export function App({ content }: { content: HomeContent }) {
  useReveal()

  return (
    <main className="site-shell">
      <InstallHeader />
      <Hero messages={content.messages} />
      <LandscapeValuesSection content={content.openSource} />
      <NotesSection content={content} />
      <ThemeSection content={content.theme} />
      <FooterSection content={content.footer} />
    </main>
  )
}

function InstallHeader() {
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
      <a className="install-header-button" href="#top">
        <Download aria-hidden="true" />
        <span>Install on Linux</span>
      </a>
      <nav className="install-header-nav" aria-label="Site">
        <a href="#notes">Documentation</a>
        <a href="#themes">Themes</a>
        <a href="#top">Releases</a>
      </nav>
    </header>
  )
}

function Hero({ messages }: { messages: SiteMessage[] }) {
  const [selectedMessageId, setSelectedMessageId] = useState(messages[0].id)
  const selectedMessage =
    messages.find((message) => message.id === selectedMessageId) ?? messages[0]

  return (
    <section className="hero" aria-labelledby="hero-title">
      <div className="client-frame is-visible" data-reveal>
        <ClientToolbar />
        <div className="client-body">
          <SidebarPreview />
          <MessageListPreview
            messages={messages}
            selectedMessageId={selectedMessage.id}
            onSelectMessage={setSelectedMessageId}
          />
          <ReaderPreview message={selectedMessage} />
        </div>
      </div>
    </section>
  )
}

function ClientToolbar() {
  return (
    <nav className="client-toolbar" aria-label="Primary">
      <div className="traffic-lights" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <button type="button" className="toolbar-chip" aria-label="Compose">
        <Mail aria-hidden="true" />
      </button>
      <button type="button" className="icon-chip" aria-label="Archive">
        <Archive aria-hidden="true" />
      </button>
      <button type="button" className="icon-chip" aria-label="Flag">
        <Flag aria-hidden="true" />
      </button>
      <div className="toolbar-spacer" />
      <div className="nav-links">
        <a href="#notes">Notes</a>
        <a href="#themes">Themes</a>
      </div>
      <div className="mock-search">
        <Search aria-hidden="true" />
        <span>Search mail</span>
        <kbd>⌘K</kbd>
      </div>
      <button type="button" className="icon-chip" aria-label="Settings">
        <Settings aria-hidden="true" />
      </button>
    </nav>
  )
}

function SidebarPreview() {
  return (
    <aside className="mock-sidebar" aria-label="Mailbox preview">
      <div className="section-label">Smart</div>
      {mailboxes.map((mailbox) => (
        <div
          className={`mailbox-row ${mailbox.active ? 'active' : ''}`}
          key={mailbox.label}
        >
          <span className={`mailbox-dot ${mailbox.color}`} />
          <span>{mailbox.label}</span>
          <span className="count">{mailbox.count}</span>
        </div>
      ))}
      <div className="section-label account-label">Accounts</div>
      <div className="account-row">
        <span className="account-stamp stalwart">S</span>
        <span>Stalwart</span>
      </div>
      <div className="account-row">
        <span className="account-stamp fastmail">F</span>
        <span>Fastmail</span>
      </div>
    </aside>
  )
}

function MessageListPreview({
  messages,
  selectedMessageId,
  onSelectMessage,
}: {
  messages: SiteMessage[]
  selectedMessageId: string
  onSelectMessage: (messageId: string) => void
}) {
  return (
    <section className="mock-list" aria-label="Conversation list preview">
      <div className="list-heading">
        <span>Subject</span>
        <span>From</span>
        <span>Date received</span>
        <span>Tags</span>
      </div>
      {messages.map((message) => (
        <button
          type="button"
          className={`message-row ${message.unread ? 'unread' : ''} ${
            message.id === selectedMessageId ? 'selected' : ''
          }`}
          key={message.id}
          aria-pressed={message.id === selectedMessageId}
          onClick={() => onSelectMessage(message.id)}
        >
          <span className={`unread-dot ${message.color}`} />
          <div className="message-subject">
            <strong>{message.subject}</strong>
            <span>{message.from}</span>
          </div>
          <time>{message.time}</time>
          <span className={`tag-pill ${message.color}`}>{message.tag}</span>
        </button>
      ))}
      <div className="list-skeleton" aria-hidden="true">
        <span />
        <span />
        <span />
        <span />
      </div>
    </section>
  )
}

function ReaderPreview({ message }: { message: SiteMessage }) {
  return (
    <section className="mock-reader" aria-labelledby="hero-title">
      <SloganTitle id="hero-title" />
      <div className="reader-message" aria-live="polite">
        <h2>{message.subject}</h2>
        <div
          className="reader-message-body"
          dangerouslySetInnerHTML={{ __html: message.html }}
        />
      </div>
    </section>
  )
}

function SloganTitle({ id }: { id?: string }) {
  return (
    <h1 className="slogan" id={id}>
      <span>Your Mail</span>
      <span>
        Delivered at Post
        <span className="letter-h">H</span>
        <span className="letter-a">a</span>
        <span className="letter-s">s</span>
        <span className="letter-t">t</span>
        <span className="letter-e">e</span>
      </span>
    </h1>
  )
}

function LandscapeValuesSection({
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

function NotesSection({ content }: { content: HomeContent }) {
  return (
    <section className="notes-section" id="notes" aria-labelledby="notes-title">
      <div className="section-header" data-reveal>
        <p className="eyebrow">Notes</p>
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

function ThemeSection({ content }: { content: HomeContent['theme'] }) {
  return (
    <section
      className="theme-section"
      id="themes"
      aria-labelledby="themes-title"
    >
      <div className="theme-copy" data-reveal>
        <p className="eyebrow">
          <Palette aria-hidden="true" />
          {content.eyebrow}
        </p>
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

function FooterSection({ content }: { content: HomeContent['footer'] }) {
  return (
    <footer className="footer-section">
      <span>{content.brand}</span>
      <div dangerouslySetInnerHTML={{ __html: content.html }} />
    </footer>
  )
}
