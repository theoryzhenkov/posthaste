import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type UIEvent as ReactUIEvent,
} from 'react'

import type {
  CommandPaletteEntry,
  SearchCandidate,
} from '@/command-search/types'
import type { MailSelection } from '@/mailState'
import { validateSearchQuery } from '@/queryLanguage'
import { normalizeAppliedSearchQuery } from '@/searchQuery'
import type { SettingsSurfaceCategory as SettingsCategory } from '@/surfaces'

import { CommandPaletteList } from './command-palette/CommandPaletteList'
import {
  COMMAND_PANEL_STORAGE_KEY,
  commandPaletteEntryValue,
  currentSearchableServerQuery,
  NO_COMMAND_PALETTE_SELECTION,
} from './command-palette/model'
import { useCommandPaletteSearch } from './command-palette/useCommandPaletteSearch'
import { usePaletteActions } from './command-palette/usePaletteActions'
import { FloatingPanel } from './FloatingPanel'
import { Command, CommandInput } from './ui/command'

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
  const { activeSelectedIndex, itemRows, search, selectedValue } =
    useCommandPaletteSearch({ hasSelectedMessage, query })

  const serverQuery = currentSearchableServerQuery(query)
  const messageProviderState = search.session.providerStates.get('messages')
  const canPreviewSearch =
    serverQuery.length > 0 &&
    messageProviderState?.status === 'done' &&
    messageProviderState.candidates.length > 0

  useEffect(() => {
    if (serverQuery.length > 0 || !hasPreviewedSearchRef.current) return
    onRejectSearchPreview()
    hasPreviewedSearchRef.current = false
  }, [onRejectSearchPreview, serverQuery])

  useEffect(() => {
    if (!canPreviewSearch) return
    hasPreviewedSearchRef.current = true
    onPreviewSearch(serverQuery)
  }, [canPreviewSearch, onPreviewSearch, serverQuery])

  function handleQueryChange(value: string) {
    setQuery(value)
    search.select(null)
  }

  function rejectPreviewedSearch() {
    if (!hasPreviewedSearchRef.current) return
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
    if (!normalized || validateSearchQuery(query).state !== 'valid') return
    onApplySearch(normalized)
    hasPreviewedSearchRef.current = false
    onClose()
  }

  const paletteActionHandlers = useMemo(
    () => ({
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
      replaceQuery: (nextQuery: string) => {
        setQuery(nextQuery)
        search.select(null)
      },
    }),
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
  const executeAction = usePaletteActions(paletteActionHandlers)

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
    if (isDownKey || isUpKey) {
      event.preventDefault()
      moveSelection(isDownKey ? 1 : -1)
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

  function moveSelection(delta: 1 | -1) {
    if (itemRows.length === 0) {
      search.select(null)
      return
    }
    if (delta === -1 && activeSelectedIndex === -1) {
      search.select(itemRows.at(-1)?.candidate.id ?? null)
      return
    }
    const nextIndex =
      delta === 1
        ? activeSelectedIndex === -1
          ? 0
          : Math.min(activeSelectedIndex + 1, itemRows.length - 1)
        : activeSelectedIndex - 1
    search.select(nextIndex < 0 ? null : itemRows[nextIndex].candidate.id)
  }

  function handleListScroll(event: ReactUIEvent<HTMLDivElement>) {
    const node = event.currentTarget
    const distanceToEnd = node.scrollHeight - node.scrollTop - node.clientHeight
    if (distanceToEnd > 120) return
    for (const [providerId, state] of search.session.providerStates) {
      if (state.nextCursor) {
        search.loadMore(providerId)
        break
      }
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
        <CommandPaletteList
          itemRowsLength={itemRows.length}
          rows={search.session.rows}
          onRunCandidate={runCandidate}
          onScroll={handleListScroll}
          onSelectNone={() => search.select(null)}
        />
      </FloatingPanel>
    </Command>
  )
}
