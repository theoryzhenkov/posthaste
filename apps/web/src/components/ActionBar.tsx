import {
  Archive,
  Clock3,
  Flag,
  Forward,
  Moon,
  PenSquare,
  Reply,
  ReplyAll,
  Search,
  Settings,
  SunMedium,
  Tag,
  Trash2,
} from 'lucide-react'
import type { RefObject } from 'react'

import { cn } from '@/lib/utils'

interface ActionBarProps {
  isDarkMode: boolean
  isFlagged: boolean
  isMessageSelected: boolean
  isSearchActive: boolean
  isSettingsOpen: boolean
  searchInputRef: RefObject<HTMLInputElement | null>
  searchQuery: string
  onArchive: () => void
  onClearSearch: () => void
  onCompose: () => void
  onFocusSearch: () => void
  onOpenCommandPalette: () => void
  onPlaceholderAction: (label: string) => void
  onReply: () => void
  onSearchBlur: () => void
  onSearchQueryChange: (query: string) => void
  onShowShortcuts: () => void
  onToggleFlag: () => void
  onToggleSettings: () => void
  onToggleTheme: () => void
  onTrash: () => void
}

interface ToolbarChipProps {
  active?: boolean
  disabled?: boolean
  hint?: string
  icon: React.ReactNode
  label?: string
  onClick: () => void
  title: string
}

function TrafficLights() {
  return (
    <div className="flex items-center gap-2">
      <span className="size-3 rounded-full bg-[#ff5f57] shadow-[inset_0_0_0_0.5px_rgba(0,0,0,0.2)]" />
      <span className="size-3 rounded-full bg-[#febc2e] shadow-[inset_0_0_0_0.5px_rgba(0,0,0,0.2)]" />
      <span className="size-3 rounded-full bg-[#28c940] shadow-[inset_0_0_0_0.5px_rgba(0,0,0,0.2)]" />
    </div>
  )
}

function Divider() {
  return <div className="mx-1.5 h-[18px] w-px bg-border-soft" />
}

function ToolbarChip({
  active,
  disabled,
  hint,
  icon,
  label,
  onClick,
  title,
}: ToolbarChipProps) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        'ph-focus-ring inline-flex h-7 shrink-0 items-center gap-1.5 rounded-[6px] px-2 text-[12px] font-medium text-chrome-foreground/70 transition-colors',
        'hover:bg-[var(--hover-bg)] hover:text-chrome-foreground disabled:opacity-35',
        label ? 'pr-2.5' : 'w-7 justify-center px-0',
        active && 'bg-brand-coral-soft text-[var(--brand-coral-deep)]',
      )}
    >
      <span className="shrink-0">{icon}</span>
      {label && <span>{label}</span>}
      {label && hint && (
        <span className="rounded-[4px] bg-black/6 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-chrome-foreground/52">
          {hint}
        </span>
      )}
    </button>
  )
}

function SearchField({
  isActive,
  searchInputRef,
  searchQuery,
  onClearSearch,
  onFocus,
  onBlur,
  onOpenCommandPalette,
  onSearchQueryChange,
}: {
  isActive: boolean
  searchInputRef: RefObject<HTMLInputElement | null>
  searchQuery: string
  onClearSearch: () => void
  onFocus: () => void
  onBlur: () => void
  onOpenCommandPalette: () => void
  onSearchQueryChange: (query: string) => void
}) {
  const expanded = isActive || searchQuery.length > 0

  if (!expanded) {
    return (
      <button
        type="button"
        onClick={onOpenCommandPalette}
        className="ph-focus-ring flex h-[26px] w-[220px] items-center gap-2 rounded-[6px] border border-border-soft bg-[var(--bg-elev)] px-2 text-left text-[12px] text-muted-foreground transition-[width,box-shadow,border-color] hover:border-border"
      >
        <Search
          size={13}
          strokeWidth={1.75}
          className="text-muted-foreground/70"
        />
        <span className="flex-1">Search mail</span>
        <span className="rounded-[4px] border border-border/80 bg-background/85 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-muted-foreground">
          ⌘K
        </span>
      </button>
    )
  }

  return (
    <div className="flex h-[26px] w-[340px] items-center gap-2 rounded-[6px] border border-ring bg-panel px-2 shadow-[0_0_0_2px_color-mix(in_oklab,var(--ring)_30%,transparent)] transition-[width,box-shadow,border-color]">
      <Search
        size={13}
        strokeWidth={1.75}
        className="text-muted-foreground/70"
      />
      <input
        ref={searchInputRef}
        autoFocus
        type="text"
        value={searchQuery}
        onFocus={onFocus}
        onBlur={() => {
          if (!searchQuery) {
            onBlur()
          }
        }}
        onChange={(event) => onSearchQueryChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            onClearSearch()
            onBlur()
            searchInputRef.current?.blur()
          }
        }}
        placeholder="from:maya tag:work date:>2026-04-01"
        className="h-full flex-1 border-0 bg-transparent font-mono text-[12px] text-foreground outline-none placeholder:text-muted-foreground/70"
      />
    </div>
  )
}

export function ActionBar({
  isDarkMode,
  isFlagged,
  isMessageSelected,
  isSearchActive,
  isSettingsOpen,
  searchInputRef,
  searchQuery,
  onArchive,
  onClearSearch,
  onCompose,
  onFocusSearch,
  onOpenCommandPalette,
  onPlaceholderAction,
  onReply,
  onSearchBlur,
  onSearchQueryChange,
  onShowShortcuts,
  onToggleFlag,
  onToggleSettings,
  onToggleTheme,
  onTrash,
}: ActionBarProps) {
  return (
    <header className="flex h-[42px] shrink-0 items-center gap-1 border-b border-border-soft bg-chrome px-3 text-chrome-foreground">
      <TrafficLights />
      <div className="w-4" />

      <ToolbarChip
        hint="⌘N"
        icon={<PenSquare size={14} strokeWidth={1.6} />}
        label="Compose"
        onClick={onCompose}
        title="Compose"
      />
      <Divider />
      <ToolbarChip
        hint="⌘R"
        disabled={!isMessageSelected}
        icon={<Reply size={14} strokeWidth={1.6} />}
        onClick={onReply}
        title="Reply"
      />
      <ToolbarChip
        hint="⇧⌘R"
        disabled={!isMessageSelected}
        icon={<ReplyAll size={14} strokeWidth={1.6} />}
        onClick={() => onPlaceholderAction('Reply all')}
        title="Reply all"
      />
      <ToolbarChip
        hint="⇧⌘F"
        icon={<Forward size={14} strokeWidth={1.6} />}
        onClick={() => onPlaceholderAction('Forward')}
        title="Forward"
      />
      <Divider />
      <ToolbarChip
        hint="E"
        disabled={!isMessageSelected}
        icon={<Archive size={14} strokeWidth={1.6} />}
        onClick={onArchive}
        title="Archive"
      />
      <ToolbarChip
        hint="⌫"
        disabled={!isMessageSelected}
        icon={<Trash2 size={14} strokeWidth={1.6} />}
        onClick={onTrash}
        title="Trash"
      />
      <ToolbarChip
        active={isFlagged}
        hint="⇧⌘L"
        disabled={!isMessageSelected}
        icon={<Flag size={14} strokeWidth={1.6} />}
        onClick={onToggleFlag}
        title="Flag"
      />
      <ToolbarChip
        hint="H"
        icon={<Clock3 size={14} strokeWidth={1.6} />}
        onClick={() => onPlaceholderAction('Snooze')}
        title="Snooze"
      />
      <ToolbarChip
        hint="L"
        icon={<Tag size={14} strokeWidth={1.6} />}
        onClick={() => onPlaceholderAction('Tag')}
        title="Tag"
      />

      <div className="flex-1" />

      <SearchField
        isActive={isSearchActive}
        searchInputRef={searchInputRef}
        searchQuery={searchQuery}
        onClearSearch={onClearSearch}
        onFocus={onFocusSearch}
        onBlur={onSearchBlur}
        onOpenCommandPalette={onOpenCommandPalette}
        onSearchQueryChange={onSearchQueryChange}
      />

      <button
        type="button"
        className="ph-focus-ring ml-1 flex size-7 items-center justify-center rounded-[6px] text-[13px] font-bold text-chrome-foreground/60 transition-colors hover:bg-[var(--hover-bg)] hover:text-chrome-foreground"
        onClick={onShowShortcuts}
        title="Keyboard shortcuts (?)"
      >
        ?
      </button>
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
