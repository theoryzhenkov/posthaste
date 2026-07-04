import {
  useMemo,
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
  NO_COMMAND_PALETTE_SELECTION,
  resolvePaletteEnter,
} from './command-palette/model'
import { useCommandPaletteSearch } from './command-palette/useCommandPaletteSearch'
import { usePaletteActions } from './command-palette/usePaletteActions'
import { FloatingPanel } from './FloatingPanel'
import { Command, CommandInput } from './ui/command'

interface CommandPaletteProps {
  hasSelectedMessage: boolean
  selectedMessageTags: readonly string[]
  onAddTag: (tag: string) => void
  onApplySearch: (query: string) => void
  onArchive: () => void
  onClose: () => void
  onCompose: () => void
  onOpenSettings: (category?: SettingsCategory) => void
  onOpenShortcuts: () => void
  onPlaceholderAction: (label: string) => void
  onRemoveTag: (tag: string) => void
  onReply: () => void
  onSelectMessage: (selection: MailSelection) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onToggleFlag: () => void
}

export function CommandPalette({
  hasSelectedMessage,
  selectedMessageTags,
  onAddTag,
  onApplySearch,
  onArchive,
  onClose,
  onCompose,
  onOpenSettings,
  onOpenShortcuts,
  onPlaceholderAction,
  onRemoveTag,
  onReply,
  onSelectMessage,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onToggleFlag,
}: CommandPaletteProps) {
  const [query, setQuery] = useState('')
  const { activeSelectedIndex, itemRows, search, selectedValue } =
    useCommandPaletteSearch({ hasSelectedMessage, query, selectedMessageTags })

  function handleQueryChange(value: string) {
    setQuery(value)
    search.select(null)
  }

  function closeWithoutApplyingQuery() {
    search.cancel()
    onClose()
  }

  // The typed query never touches the underlying mail view while typing — it
  // only filters palette candidates — so a selected message stays in scope. The
  // app-wide mail-view filter is applied here, only on Shift+Enter; a plain
  // Enter navigates into the in-pane results instead (see handlePaletteKeyDown).
  function applyCurrentQuery() {
    const normalized = normalizeAppliedSearchQuery(query)
    if (!normalized || validateSearchQuery(query).state !== 'valid') return
    onApplySearch(normalized)
    onClose()
  }

  const paletteActionHandlers = useMemo(
    () => ({
      onAddTag,
      onApplySearch,
      onArchive,
      onCompose,
      onOpenSettings,
      onOpenShortcuts,
      onPlaceholderAction,
      onRemoveTag,
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
      onAddTag,
      onApplySearch,
      onArchive,
      onCompose,
      onOpenSettings,
      onOpenShortcuts,
      onPlaceholderAction,
      onRemoveTag,
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
      const action = resolvePaletteEnter({
        shiftKey: event.shiftKey,
        hasHighlightedItem: activeSelectedIndex !== -1,
        hasItems: itemRows.length > 0,
      })
      switch (action) {
        case 'apply':
          applyCurrentQuery()
          break
        case 'run':
          runCandidate(itemRows[activeSelectedIndex].candidate)
          break
        case 'navigate':
          moveSelection(1)
          break
        case 'none':
          break
      }
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
