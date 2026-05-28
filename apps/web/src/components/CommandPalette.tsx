import { useQuery } from '@tanstack/react-query'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react'

import { fetchSearchMessages, fetchSidebar } from '@/api/client'
import type { MessageSummary } from '@/api/types'
import {
  commandPaletteEntryValue,
  NO_COMMAND_PALETTE_SELECTION,
  type CommandPaletteEntry,
  type SettingsCategory,
  useCommandPaletteResults,
} from '@/hooks/useCommandPaletteResults'
import { useDebouncedValue } from '@/hooks/useDebouncedValue'
import { createOperationContext } from '@/observability'
import { queryKeys } from '@/queryKeys'
import { validateSearchQuery } from '@/queryLanguage'
import { normalizeAppliedSearchQuery } from '@/searchQuery'

import { FloatingPanel } from './FloatingPanel'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from './ui/command'

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
  onSelectMessage: (message: MessageSummary) => void
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
const EMPTY_MESSAGE_RESULTS: MessageSummary[] = []

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
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null)
  const hasPreviewedSearchRef = useRef(false)
  const queryValidation = useMemo(() => validateSearchQuery(query), [query])
  const serverQuery =
    queryValidation.state === 'valid' ? normalizeAppliedSearchQuery(query) : ''
  const debouncedServerQuery = useDebouncedValue(serverQuery, 180)
  const searchPreviewOperation = useMemo(
    () =>
      debouncedServerQuery
        ? createOperationContext('mail.search.preview', 'command-palette')
        : undefined,
    [debouncedServerQuery],
  )
  const { data: sidebar } = useQuery({
    queryKey: ['sidebar'],
    queryFn: fetchSidebar,
  })
  const searchMessagesQuery = useQuery({
    queryKey: [
      ...queryKeys.messagesRoot,
      'global-search',
      debouncedServerQuery,
    ] as const,
    queryFn: ({ signal }) =>
      fetchSearchMessages(debouncedServerQuery, {
        limit: 8,
        signal,
        operation: searchPreviewOperation,
      }),
    enabled: debouncedServerQuery.length > 0,
  })
  const canPreviewSearch =
    debouncedServerQuery.length > 0 && searchMessagesQuery.isSuccess
  const hasPreviewSearchError = searchMessagesQuery.isError
  const canPreviewCurrentQuery =
    serverQuery.length > 0 && debouncedServerQuery === serverQuery

  useEffect(() => {
    if (serverQuery.length > 0 || !hasPreviewedSearchRef.current) {
      return
    }
    onRejectSearchPreview()
    hasPreviewedSearchRef.current = false
  }, [onRejectSearchPreview, serverQuery])

  useEffect(() => {
    if (!canPreviewCurrentQuery || !canPreviewSearch || hasPreviewSearchError) {
      return
    }
    hasPreviewedSearchRef.current = true
    onPreviewSearch(debouncedServerQuery)
  }, [
    canPreviewCurrentQuery,
    canPreviewSearch,
    debouncedServerQuery,
    hasPreviewSearchError,
    onPreviewSearch,
  ])

  const cachedMessages =
    searchMessagesQuery.data?.items ?? EMPTY_MESSAGE_RESULTS

  const replaceQuery = useCallback((nextQuery: string) => {
    setQuery(nextQuery)
    setSelectedIndex(null)
  }, [])
  const results = useCommandPaletteResults({
    cachedMessages,
    hasSelectedMessage,
    onApplySearch,
    onArchive,
    onCompose,
    onOpenSettings,
    onOpenShortcuts,
    onPlaceholderAction,
    onReplaceQuery: replaceQuery,
    onReply,
    onSelectMessage,
    onSelectSmartMailbox,
    onSelectSourceMailbox,
    onToggleFlag,
    query,
    sidebar,
  })

  const flatEntries = useMemo(
    () => results.flatMap((group) => group.items),
    [results],
  )
  const activeSelectedIndex =
    selectedIndex !== null && selectedIndex < flatEntries.length
      ? selectedIndex
      : null
  const selectedValue =
    activeSelectedIndex === null
      ? NO_COMMAND_PALETTE_SELECTION
      : commandPaletteEntryValue(flatEntries[activeSelectedIndex])

  function handleQueryChange(value: string) {
    setQuery(value)
    setSelectedIndex(null)
  }

  function runEntry(entry: CommandPaletteEntry) {
    entry.onSelect()
    if (entry.closeOnSelect !== false) {
      onClose()
    }
  }

  function applyCurrentQuery() {
    const normalized = normalizeAppliedSearchQuery(query)
    if (!normalized || queryValidation.state !== 'valid') {
      return
    }
    onApplySearch(normalized)
    hasPreviewedSearchRef.current = false
    onClose()
  }

  function handlePaletteKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    const isDownKey =
      event.key === 'ArrowDown' ||
      (event.key === 'j' && (activeSelectedIndex !== null || event.ctrlKey))
    const isUpKey =
      event.key === 'ArrowUp' ||
      (event.key === 'k' && (activeSelectedIndex !== null || event.ctrlKey))

    if (event.key === 'Escape') {
      event.preventDefault()
      if (hasPreviewedSearchRef.current) {
        onRejectSearchPreview()
        hasPreviewedSearchRef.current = false
      }
      onClose()
      return
    }

    if (isDownKey) {
      event.preventDefault()
      if (flatEntries.length === 0) {
        setSelectedIndex(null)
        return
      }
      setSelectedIndex((current) => {
        const bounded =
          current !== null && current < flatEntries.length ? current : null
        return bounded === null
          ? 0
          : Math.min(bounded + 1, flatEntries.length - 1)
      })
      return
    }

    if (isUpKey) {
      event.preventDefault()
      if (flatEntries.length === 0) {
        setSelectedIndex(null)
        return
      }
      setSelectedIndex((current) => {
        const bounded =
          current !== null && current < flatEntries.length ? current : null
        if (bounded === null) {
          return flatEntries.length - 1
        }
        return bounded === 0 ? null : bounded - 1
      })
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      if (event.shiftKey || event.altKey) {
        applyCurrentQuery()
        return
      }
      if (activeSelectedIndex !== null) {
        runEntry(flatEntries[activeSelectedIndex])
        return
      }
      applyCurrentQuery()
    }
  }

  return (
    <Command
      shouldFilter={false}
      loop={false}
      value={selectedValue}
      className="contents"
      onValueChange={(value) => {
        const nextIndex = flatEntries.findIndex(
          (entry) => commandPaletteEntryValue(entry) === value,
        )
        setSelectedIndex(nextIndex === -1 ? null : nextIndex)
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
        onClose={onClose}
      >
        <CommandList className="ph-scroll max-h-[min(440px,calc(100vh-170px))] px-0 py-1.5">
          <CommandEmpty>No results. Try a different query.</CommandEmpty>
          {flatEntries.length > 0 && (
            <CommandItem
              aria-hidden="true"
              value={NO_COMMAND_PALETTE_SELECTION}
              className="hidden"
              onSelect={() => setSelectedIndex(null)}
            />
          )}
          {results.map((group) => (
            <CommandGroup
              key={group.label}
              heading={group.label}
              className="py-1"
            >
              {group.items.map((item) => (
                <CommandItem
                  key={item.id}
                  value={commandPaletteEntryValue(item)}
                  className="mx-0 px-4 py-2.5 text-foreground data-[selected=true]:bg-[var(--hover-bg)]"
                  onSelect={() => {
                    runEntry(item)
                  }}
                >
                  <span className="flex size-4 shrink-0 items-center justify-center">
                    {item.icon}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{item.label}</span>
                  {item.sub && (
                    <span className="max-w-[14rem] truncate text-[12px] text-muted-foreground">
                      {item.sub}
                    </span>
                  )}
                </CommandItem>
              ))}
            </CommandGroup>
          ))}
        </CommandList>
      </FloatingPanel>
    </Command>
  )
}
