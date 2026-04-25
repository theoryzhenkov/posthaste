import {
  Archive,
  Clock3,
  Command,
  Flag,
  Forward,
  Maximize2,
  Moon,
  PenSquare,
  Reply,
  ReplyAll,
  Settings,
  SunMedium,
  Tag,
  Trash2,
  X,
} from 'lucide-react'

import { cn } from '@/lib/utils'

interface ActionBarProps {
  isDarkMode: boolean
  isFlagged: boolean
  isMessageSelected: boolean
  isSettingsOpen: boolean
  searchQuery: string
  onArchive: () => void
  onClearSearch: () => void
  onCompose: () => void
  onOpenCommandPalette: () => void
  onOpenFocusedMessage: () => void
  onPlaceholderAction: (label: string) => void
  onReply: () => void
  onShowShortcuts: () => void
  onTag: () => void
  onToggleFlag: () => void
  onToggleSettings: () => void
  onToggleTheme: () => void
  onTrash: () => void
}

interface ToolbarChipProps {
  active?: boolean
  disabled?: boolean
  tagEditorTrigger?: boolean
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
  tagEditorTrigger,
  hint,
  icon,
  label,
  onClick,
  title,
}: ToolbarChipProps) {
  return (
    <button
      type="button"
      data-tag-editor-trigger={tagEditorTrigger ? 'true' : undefined}
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
  isFlagged,
  isMessageSelected,
  isSettingsOpen,
  searchQuery,
  onArchive,
  onClearSearch,
  onCompose,
  onOpenCommandPalette,
  onOpenFocusedMessage,
  onPlaceholderAction,
  onReply,
  onShowShortcuts,
  onTag,
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
        icon={<PenSquare size={14} strokeWidth={1.6} />}
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
        disabled={!isMessageSelected}
        tagEditorTrigger
        icon={<Tag size={14} strokeWidth={1.6} />}
        onClick={onTag}
        title="Tag"
      />
      <ToolbarChip
        hint="O"
        disabled={!isMessageSelected}
        icon={<Maximize2 size={14} strokeWidth={1.6} />}
        onClick={onOpenFocusedMessage}
        title="Open message"
      />

      <div className="flex-1" />

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
