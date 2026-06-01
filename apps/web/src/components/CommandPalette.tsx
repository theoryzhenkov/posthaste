import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type UIEvent as ReactUIEvent,
} from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { createCommandProviders } from '@/command-search/providers'
import { recentCachedMessages } from '@/command-search/recentMessages'
import { useCommandSearch } from '@/command-search/useCommandSearch'
import type {
  CommandPaletteEntry,
  PaletteAction,
  PaletteRow,
  RankingContext,
  SearchCandidate,
} from '@/command-search/types'
import { useMailboxNavigationReadModels } from '@/mailboxNavigationReadModels'
import type { MailSelection } from '@/mailState'
import { validateSearchQuery } from '@/queryLanguage'
import { normalizeAppliedSearchQuery } from '@/searchQuery'
import type { SettingsSurfaceCategory as SettingsCategory } from '@/surfaces'

import { FloatingPanel } from './FloatingPanel'
import { Command, CommandInput, CommandItem, CommandList } from './ui/command'

interface CommandPaletteProps {
  hasSelectedMessage: boolean
  onApplySearch: (query: string) => void
  onArchive: () => void
  onClose: () => void
  onCompose: () => void
  onOpenSettings: (category?: SettingsCategory) => void
  onOpenShortcuts: () => void
  onPlaceholderAction: (label: string) => void
  onRejectSearchPreview: () => void
  onReply: () => void
  onSelectMessage: (selection: MailSelection) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onPreviewSearch: (query: string) => void
  onToggleFlag: () => void
}

const COMMAND_PANEL_STORAGE_KEY = 'posthaste.commandPalette.panelOffset'
const NO_COMMAND_PALETTE_SELECTION = '__posthaste_no_selection__'

function commandPaletteEntryValue(candidate: SearchCandidate): string {
  return `candidate:${candidate.id}`
}

function emptyCounter() {
  return { halfLifeMs: 7 * 24 * 60 * 60 * 1000, entries: {} }
}

function isItemRow(
  row: PaletteRow,
): row is Extract<PaletteRow, { kind: 'item' }> {
  return row.kind === 'item'
}

function currentSearchableServerQuery(query: string): string {
  const validation = validateSearchQuery(query)
  if (validation.state !== 'valid') return ''
  const normalized = normalizeAppliedSearchQuery(query)
  if (!normalized) return ''
  if (normalized.includes(':')) return normalized
  return normalized.length >= 2 ? normalized : ''
}

function createRankingContext(input: {
  hasSelectedMessage: boolean
}): RankingContext {
  return {
    now: Date.now(),
    app: {
      route: input.hasSelectedMessage ? 'thread' : 'mailbox',
      hasSelectedMessage: input.hasSelectedMessage,
    },
    session: {
      paletteOpenReason: 'keyboard',
    },
    user: {
      recentCommands: emptyCounter(),
      recentEntities: emptyCounter(),
      frequentCommands: emptyCounter(),
      frequentMailboxes: emptyCounter(),
      frequentContacts: emptyCounter(),
      pinnedCommands: [],
      pinnedMailboxes: [],
    },
  }
}

export function CommandPalette({
  hasSelectedMessage,
  onApplySearch,
  onArchive,
  onClose,
  onCompose,
  onOpenSettings,
  onOpenShortcuts,
  onPlaceholderAction,
  onPreviewSearch,
  onRejectSearchPreview,
  onReply,
  onSelectMessage,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onToggleFlag,
}: CommandPaletteProps) {
  const [query, setQuery] = useState('')
  const hasPreviewedSearchRef = useRef(false)
  const queryClient = useQueryClient()
  const readModels = useMailboxNavigationReadModels()
  // Snapshot recents once when the palette opens. They come from already-loaded
  // React Query pages (not a fetch), so a stable snapshot for the session is
  // intentional — we do not want recents reshuffling as background pages load.
  const recentMessages = useMemo(
    () => recentCachedMessages(queryClient),
    [queryClient],
  )
  const readModelKey = useMemo(
    () =>
      JSON.stringify({
        smartMailboxes: readModels.smartMailboxes.map((item) => item.id),
        sources: readModels.sources.map((source) => ({
          id: source.id,
          mailboxes: source.mailboxes.map((mailbox) => mailbox.id),
        })),
        tags: readModels.tags.map((tag) => tag.name),
      }),
    [readModels.smartMailboxes, readModels.sources, readModels.tags],
  )
  const providers = useMemo(
    () =>
      createCommandProviders({
        readModels,
        recentMessages,
      }),
    // readModelKey intentionally collapses unstable React Query wrapper arrays
    // into the domain IDs that affect provider candidates.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [readModelKey, recentMessages],
  )
  const rankingContext = useMemo(
    () => createRankingContext({ hasSelectedMessage }),
    [hasSelectedMessage],
  )
  const search = useCommandSearch({
    query,
    context: rankingContext,
    providers,
  })
  const itemRows = useMemo(
    () => search.session.rows.filter(isItemRow),
    [search.session.rows],
  )
  const activeSelectedIndex = search.session.selectedCandidateId
    ? itemRows.findIndex(
        (row) => row.candidate.id === search.session.selectedCandidateId,
      )
    : -1
  const selectedValue =
    activeSelectedIndex === -1
      ? NO_COMMAND_PALETTE_SELECTION
      : commandPaletteEntryValue(itemRows[activeSelectedIndex].candidate)

  const serverQuery = currentSearchableServerQuery(query)
  const messageProviderState = search.session.providerStates.get('messages')
  const canPreviewSearch =
    serverQuery.length > 0 &&
    messageProviderState?.status === 'done' &&
    messageProviderState.candidates.length > 0

  useEffect(() => {
    if (serverQuery.length > 0 || !hasPreviewedSearchRef.current) {
      return
    }
    onRejectSearchPreview()
    hasPreviewedSearchRef.current = false
  }, [onRejectSearchPreview, serverQuery])

  useEffect(() => {
    if (!canPreviewSearch) {
      return
    }
    hasPreviewedSearchRef.current = true
    onPreviewSearch(serverQuery)
  }, [canPreviewSearch, onPreviewSearch, serverQuery])

  function handleQueryChange(value: string) {
    setQuery(value)
    search.select(null)
  }

  function rejectPreviewedSearch() {
    if (!hasPreviewedSearchRef.current) {
      return
    }
    onRejectSearchPreview()
    hasPreviewedSearchRef.current = false
  }

  function closeWithoutApplyingQuery() {
    rejectPreviewedSearch()
    search.cancel()
    onClose()
  }

  function applyCurrentQuery() {
    const normalized = normalizeAppliedSearchQuery(query)
    if (!normalized || validateSearchQuery(query).state !== 'valid') {
      return
    }
    onApplySearch(normalized)
    hasPreviewedSearchRef.current = false
    onClose()
  }

  const executeAction = useCallback(
    (action: PaletteAction) => {
      switch (action.kind) {
        case 'command':
          switch (action.commandId) {
            case 'compose':
              onCompose()
              break
            case 'reply':
              onReply()
              break
            case 'archive':
              onArchive()
              break
            case 'flag':
              onToggleFlag()
              break
            case 'shortcuts':
              onOpenShortcuts()
              break
            case 'snooze':
              onPlaceholderAction('Snooze')
              break
            case 'newSmart':
            case 'newRule':
              onOpenSettings('mailboxes')
              break
            case 'settings':
              onOpenSettings()
              break
            case 'account':
              onOpenSettings('accounts')
              break
          }
          break
        case 'apply-query':
          onApplySearch(action.query)
          break
        case 'replace-query':
          setQuery(action.query)
          search.select(null)
          break
        case 'open-source-mailbox':
          onSelectSourceMailbox(action.sourceId, action.mailboxId, action.name)
          break
        case 'open-smart-mailbox':
          onSelectSmartMailbox(action.smartMailboxId, action.name)
          break
        case 'open-message':
          if (action.mailboxHint) {
            onSelectSourceMailbox(
              action.sourceId,
              action.mailboxHint.mailboxId,
              action.mailboxHint.name,
            )
          }
          onSelectMessage({
            conversationId: action.conversationId,
            sourceId: action.sourceId,
            messageId: action.messageId,
          })
          break
        case 'open-settings':
          onOpenSettings(action.category)
          break
        case 'open-compose':
          onCompose()
          break
        case 'open-contact':
          onApplySearch(action.query)
          break
        case 'noop':
          onPlaceholderAction(action.label)
          break
      }
    },
    [
      onApplySearch,
      onArchive,
      onCompose,
      onOpenSettings,
      onOpenShortcuts,
      onPlaceholderAction,
      onReply,
      onSelectMessage,
      onSelectSmartMailbox,
      onSelectSourceMailbox,
      onToggleFlag,
      search,
    ],
  )

  function runEntry(entry: CommandPaletteEntry) {
    if (entry.action.kind !== 'replace-query') {
      rejectPreviewedSearch()
    }
    executeAction(entry.action)
    if (entry.closeOnSelect !== false) {
      onClose()
    }
  }

  function runCandidate(candidate: SearchCandidate) {
    runEntry(candidate.entry)
  }

  function handlePaletteKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const isDownKey =
      event.key === 'ArrowDown' ||
      (event.key === 'j' && (activeSelectedIndex !== -1 || event.ctrlKey))
    const isUpKey =
      event.key === 'ArrowUp' ||
      (event.key === 'k' && (activeSelectedIndex !== -1 || event.ctrlKey))

    if (event.key === 'Escape') {
      event.preventDefault()
      closeWithoutApplyingQuery()
      return
    }

    if (isDownKey) {
      event.preventDefault()
      if (itemRows.length === 0) {
        search.select(null)
        return
      }
      const nextIndex =
        activeSelectedIndex === -1
          ? 0
          : Math.min(activeSelectedIndex + 1, itemRows.length - 1)
      search.select(itemRows[nextIndex].candidate.id)
      return
    }

    if (isUpKey) {
      event.preventDefault()
      if (itemRows.length === 0 || activeSelectedIndex === -1) {
        search.select(itemRows.at(-1)?.candidate.id ?? null)
        return
      }
      const nextIndex = activeSelectedIndex - 1
      search.select(nextIndex < 0 ? null : itemRows[nextIndex].candidate.id)
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      if (event.shiftKey || event.altKey) {
        applyCurrentQuery()
        return
      }
      if (activeSelectedIndex !== -1) {
        runCandidate(itemRows[activeSelectedIndex].candidate)
        return
      }
      applyCurrentQuery()
    }
  }

  function handleListScroll(event: ReactUIEvent<HTMLDivElement>) {
    const node = event.currentTarget
    const distanceToEnd = node.scrollHeight - node.scrollTop - node.clientHeight
    if (distanceToEnd > 120) {
      return
    }
    for (const [providerId, state] of search.session.providerStates) {
      if (state.nextCursor) {
        search.loadMore(providerId)
        break
      }
    }
  }

  function renderRow(row: PaletteRow) {
    switch (row.kind) {
      case 'section':
        return (
          <div
            key={row.id}
            className="px-4 py-2 font-mono text-[10px] font-semibold tracking-[0.22em] text-muted-foreground/80 uppercase"
          >
            {row.label}
          </div>
        )
      case 'item':
        return (
          <CommandItem
            key={row.id}
            value={commandPaletteEntryValue(row.candidate)}
            className="mx-0 px-4 py-2.5 text-foreground data-[selected=true]:bg-[var(--hover-bg)]"
            onSelect={() => runCandidate(row.candidate)}
          >
            <span className="flex size-4 shrink-0 items-center justify-center">
              {row.candidate.entry.icon}
            </span>
            <span className="min-w-0 flex-1 truncate">
              {row.candidate.entry.label}
            </span>
            {row.candidate.entry.subtitle && (
              <span className="max-w-[14rem] truncate text-[12px] text-muted-foreground">
                {row.candidate.entry.subtitle}
              </span>
            )}
          </CommandItem>
        )
      case 'loading':
        return (
          <div key={row.id} className="px-4 py-2 text-sm text-muted-foreground">
            {row.label}
          </div>
        )
      case 'empty':
        return (
          <div key={row.id} className="px-4 py-2 text-sm text-muted-foreground">
            {row.label}
          </div>
        )
      case 'error':
        return (
          <div key={row.id} className="px-4 py-2 text-sm text-destructive">
            {row.message}
          </div>
        )
    }
  }

  return (
    <Command
      shouldFilter={false}
      loop={false}
      value={selectedValue}
      className="contents"
      onValueChange={(value) => {
        if (value === NO_COMMAND_PALETTE_SELECTION) {
          search.select(null)
          return
        }
        const next = itemRows.find(
          (row) => commandPaletteEntryValue(row.candidate) === value,
        )
        search.select(next?.candidate.id ?? null)
      }}
      onKeyDown={handlePaletteKeyDown}
    >
      <FloatingPanel
        panelLabel="command palette"
        storageKey={COMMAND_PANEL_STORAGE_KEY}
        closeIgnoreSelector="[data-command-search-trigger='true']"
        sizePreset="command"
        header={
          <CommandInput
            autoFocus
            value={query}
            onValueChange={handleQueryChange}
            placeholder="Search messages, contacts, commands..."
            wrapperClassName="min-w-0 flex-1 h-12 px-3"
          />
        }
        onClose={closeWithoutApplyingQuery}
      >
        <CommandList
          className="ph-scroll max-h-[min(440px,calc(100vh-170px))] px-0 py-1.5"
          onScroll={handleListScroll}
        >
          {itemRows.length > 0 && (
            <CommandItem
              aria-hidden="true"
              value={NO_COMMAND_PALETTE_SELECTION}
              className="hidden"
              onSelect={() => search.select(null)}
            />
          )}
          {search.session.rows.length === 0 ? (
            <div className="py-10 text-center text-sm text-muted-foreground">
              No results. Try a different query.
            </div>
          ) : (
            search.session.rows.map(renderRow)
          )}
        </CommandList>
      </FloatingPanel>
    </Command>
  )
}
