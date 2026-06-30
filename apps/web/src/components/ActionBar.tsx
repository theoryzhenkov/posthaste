import { Command, Moon, PenSquare, Settings, SunMedium, X } from 'lucide-react'

import { cn } from '@/lib/utils'
import { NotificationsButton } from './NotificationsButton'
import { TrafficLightInset, WINDOW_TITLEBAR_HEIGHT } from './WindowChrome'

/**
 * Top window chrome: global controls only.
 *
 * Message-level actions (reply, forward, archive, trash, flag, snooze, tag,
 * open) live in the message detail header, where they are only meaningful once
 * a message is selected. The keyboard shortcuts for those actions are handled
 * globally by `useGlobalMailShortcuts` and remain available regardless.
 */
interface ActionBarProps {
  isDarkMode: boolean
  isSettingsOpen: boolean
  searchQuery: string
  onClearSearch: () => void
  onCompose: () => void
  onOpenCommandPalette: () => void
  onShowShortcuts: () => void
  onToggleSettings: () => void
  onToggleTheme: () => void
}

function ToolbarChip({
  icon,
  onClick,
  title,
}: {
  icon: React.ReactNode
  onClick: () => void
  title: string
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={cn(
        'ph-focus-ring inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-[6px] px-0 text-chrome-foreground/70 transition-colors',
        'hover:bg-[var(--hover-bg)] hover:text-chrome-foreground',
      )}
    >
      <span className="shrink-0">{icon}</span>
    </button>
  )
}

function CommandSearchControl({
  searchQuery,
  onClearSearch,
  onOpenCommandPalette,
}: {
  searchQuery: string
  onClearSearch: () => void
  onOpenCommandPalette: () => void
}) {
  const hasFilter = searchQuery.trim().length > 0

  return (
    <div className="flex min-w-0 items-center gap-2">
      <button
        type="button"
        data-command-search-trigger="true"
        onClick={onOpenCommandPalette}
        title="Command search"
        className={cn(
          'ph-focus-ring flex size-7 shrink-0 items-center justify-center rounded-[6px] border border-border-soft bg-[var(--bg-elev)] text-chrome-foreground/62 transition-colors hover:border-border hover:bg-[var(--hover-bg)] hover:text-chrome-foreground',
          hasFilter && 'border-ring text-chrome-foreground',
        )}
      >
        <Command size={14} strokeWidth={1.7} />
      </button>
      {hasFilter && (
        <span className="flex h-7 min-w-0 max-w-[24rem] items-center gap-1.5 rounded-[6px] border border-ring/45 bg-panel px-2 font-mono text-[11px] text-foreground shadow-[0_0_0_2px_color-mix(in_oklab,var(--ring)_18%,transparent)]">
          <span className="min-w-0 truncate">{searchQuery}</span>
          <button
            type="button"
            aria-label="Clear active filter"
            onClick={onClearSearch}
            className="ph-focus-ring -mr-1 flex size-5 shrink-0 items-center justify-center rounded-[4px] text-muted-foreground transition-colors hover:bg-[var(--hover-bg)] hover:text-foreground"
          >
            <X size={12} strokeWidth={1.8} />
          </button>
        </span>
      )}
    </div>
  )
}

export function ActionBar({
  isDarkMode,
  isSettingsOpen,
  searchQuery,
  onClearSearch,
  onCompose,
  onOpenCommandPalette,
  onShowShortcuts,
  onToggleSettings,
  onToggleTheme,
}: ActionBarProps) {
  return (
    <header
      className="flex shrink-0 items-center gap-1 border-b border-border-soft bg-chrome px-3 text-chrome-foreground"
      style={{ height: WINDOW_TITLEBAR_HEIGHT }}
    >
      <TrafficLightInset />

      <ToolbarChip
        icon={<PenSquare size={14} strokeWidth={1.6} />}
        onClick={onCompose}
        title="Compose"
      />

      <div data-tauri-drag-region className="flex-1 self-stretch" />

      <CommandSearchControl
        searchQuery={searchQuery}
        onClearSearch={onClearSearch}
        onOpenCommandPalette={onOpenCommandPalette}
      />

      <button
        type="button"
        data-shortcut-reference-trigger="true"
        className="ph-focus-ring ml-1 flex size-7 items-center justify-center rounded-[6px] text-[13px] font-bold text-chrome-foreground/60 transition-colors hover:bg-[var(--hover-bg)] hover:text-chrome-foreground"
        onClick={onShowShortcuts}
        title="Keyboard shortcuts (?)"
      >
        ?
      </button>
      <NotificationsButton />
      <button
        type="button"
        className={cn(
          'ph-focus-ring flex size-7 items-center justify-center rounded-[6px] text-chrome-foreground/60 transition-colors hover:bg-[var(--hover-bg)] hover:text-chrome-foreground',
          isSettingsOpen && 'bg-[var(--hover-bg)] text-chrome-foreground',
        )}
        onClick={onToggleSettings}
        title="Settings (⌘,)"
      >
        <Settings size={14} strokeWidth={1.6} />
      </button>
      <button
        type="button"
        className="ph-focus-ring flex size-7 items-center justify-center rounded-[6px] text-chrome-foreground/60 transition-colors hover:bg-[var(--hover-bg)] hover:text-chrome-foreground"
        onClick={onToggleTheme}
        title="Toggle theme"
      >
        {isDarkMode ? (
          <SunMedium size={14} strokeWidth={1.6} />
        ) : (
          <Moon size={14} strokeWidth={1.6} />
        )}
      </button>
    </header>
  )
}
