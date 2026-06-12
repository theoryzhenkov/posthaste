import { Archive, Flag } from 'lucide-react'
import type { MockTheme } from './types'

export function ClientToolbar({
  canArchive,
  isFlagged,
  isMessageSelected,
  selectedTheme,
  isSecretThemeUnlocked,
  onArchive,
  onSelectTheme,
  onToggleFlag,
}: {
  canArchive: boolean
  isFlagged: boolean
  isMessageSelected: boolean
  selectedTheme: MockTheme
  isSecretThemeUnlocked: boolean
  onArchive: () => void
  onSelectTheme: (theme: MockTheme) => void
  onToggleFlag: () => void
}) {
  const themes: { id: MockTheme; label: string }[] = [
    { id: 'baseline', label: 'Baseline' },
    { id: 'glass', label: 'Glass' },
    { id: 'typewriter', label: 'Typewriter' },
  ]
  if (isSecretThemeUnlocked) {
    themes.push({ id: 'pigeon', label: 'Pigeon' })
  }

  return (
    <nav className="client-toolbar" aria-label="Primary">
      <div className="traffic-lights" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <button
        type="button"
        className="toolbar-action"
        disabled={!canArchive}
        onClick={onArchive}
        title="Archive"
      >
        <Archive aria-hidden="true" />
      </button>
      <button
        type="button"
        className={`toolbar-action ${isFlagged ? 'active' : ''}`}
        disabled={!isMessageSelected}
        onClick={onToggleFlag}
        title="Flag"
      >
        <Flag aria-hidden="true" />
      </button>
      <div className="toolbar-spacer" />
      <div className="theme-switcher" aria-label="Mock theme switcher">
        {themes.map((theme) => (
          <button
            type="button"
            className={`theme-chip ${theme.id === selectedTheme ? 'active' : ''} ${
              theme.id === 'pigeon' ? 'secret' : ''
            }`}
            key={theme.id}
            aria-label={`${theme.label} theme`}
            title={theme.label}
            onClick={() => onSelectTheme(theme.id)}
          >
            <ThemeIcon theme={theme.id} />
          </button>
        ))}
      </div>
    </nav>
  )
}

function ThemeIcon({ theme }: { theme: MockTheme }) {
  if (theme === 'glass') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <defs>
          <linearGradient id="theme-glass" x1="4" y1="4" x2="20" y2="20">
            <stop stopColor="#9ad7ff" />
            <stop offset="0.52" stopColor="#f4b6ff" />
            <stop offset="1" stopColor="#ffe28a" />
          </linearGradient>
        </defs>
        <rect
          x="5"
          y="4"
          width="14"
          height="16"
          rx="4"
          fill="url(#theme-glass)"
          opacity="0.72"
        />
        <path
          d="M8 8h8M8 12h5"
          stroke="white"
          strokeWidth="1.8"
          strokeLinecap="round"
          opacity="0.9"
        />
      </svg>
    )
  }

  if (theme === 'typewriter') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect x="4" y="7" width="16" height="10" rx="2" fill="#f1e4c6" />
        <path
          d="M7 7V5h10v2M7 13h10M8 17h8"
          stroke="#3d3328"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
        <circle cx="8" cy="10" r="1" fill="#3d3328" />
        <circle cx="12" cy="10" r="1" fill="#3d3328" />
        <circle cx="16" cy="10" r="1" fill="#3d3328" />
      </svg>
    )
  }

  if (theme === 'pigeon') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect
          x="5"
          y="5"
          width="14"
          height="14"
          rx="4"
          fill="oklch(0.38 0.12 295)"
        />
        <path
          d="M8 9h8M8 13h5"
          stroke="oklch(0.72 0.08 295)"
          strokeWidth="1.8"
          strokeLinecap="round"
        />
        <circle cx="17" cy="17" r="2" fill="oklch(0.74 0.14 50)" />
      </svg>
    )
  }

  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect x="5" y="5" width="14" height="14" rx="4" fill="#2f3436" />
      <path
        d="M8 9h8M8 13h5"
        stroke="#f4f1ea"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <circle cx="17" cy="17" r="2" fill="#d67850" />
    </svg>
  )
}
