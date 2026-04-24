import { useQueries, useQuery } from '@tanstack/react-query'
import {
  Archive,
  Clock3,
  Keyboard,
  MessageSquareText,
  PenSquare,
  Reply,
  Settings,
  SlidersHorizontal,
  Tag,
  User,
  UserPlus,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { fetchSidebar, fetchSourceMessages } from '@/api/client'
import type { MessageSummary } from '@/api/types'
import { useDebouncedValue } from '@/hooks/useDebouncedValue'
import { renderMailboxRoleIcon, smartMailboxFallbackIcon } from '@/mailboxRoles'
import { queryKeys } from '@/queryKeys'
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

type SettingsCategory = 'general' | 'accounts' | 'mailboxes'

type PaletteCommandId =
  | 'compose'
  | 'reply'
  | 'archive'
  | 'flag'
  | 'snooze'
  | 'newSmart'
  | 'newRule'
  | 'settings'
  | 'shortcuts'
  | 'account'

type PaletteEntry =
  | {
      id: string
      kind: 'command'
      label: string
      keywords: string
      icon: React.ReactNode
      onSelect: () => void
    }
  | {
      id: string
      kind: 'message'
      label: string
      sub: string
      keywords: string
      icon: React.ReactNode
      onSelect: () => void
    }
  | {
      id: string
      kind: 'contact'
      label: string
      keywords: string
      icon: React.ReactNode
      onSelect: () => void
    }
  | {
      id: string
      kind: 'mailbox'
      label: string
      sub: string
      keywords: string
      icon: React.ReactNode
      onSelect: () => void
    }

interface CommandPaletteProps {
  hasSelectedMessage: boolean
  onApplySearch: (query: string) => void
  onArchive: () => void
  onClose: () => void
  onCompose: () => void
  onOpenSettings: (category?: SettingsCategory) => void
  onOpenShortcuts: () => void
  onPlaceholderAction: (label: string) => void
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

function normalizeQuery(value: string): string {
  return value.trim().toLowerCase()
}

function matchesQuery(query: string, text: string): boolean {
  return query.length === 0 || text.toLowerCase().includes(query)
}

type SidebarData = Awaited<ReturnType<typeof fetchSidebar>>

function resolveMessageMailbox(
  sidebar: SidebarData | undefined,
  message: MessageSummary,
) {
  const source = sidebar?.sources.find((item) => item.id === message.sourceId)
  if (!source) {
    return null
  }

  const mailbox =
    message.mailboxIds
      .map((mailboxId) =>
        source.mailboxes.find((candidate) => candidate.id === mailboxId),
      )
      .find(Boolean) ?? null

  return mailbox ? { mailbox, source } : null
}

function formatMessageSubline(
  message: MessageSummary,
  sidebar: SidebarData | undefined,
): string {
  const sender = message.fromName ?? message.fromEmail ?? 'Unknown'
  const mailbox = resolveMessageMailbox(sidebar, message)
  const location = mailbox
    ? `${mailbox.source.name} / ${mailbox.mailbox.name}`
    : message.sourceName
  const received = new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
  }).format(new Date(message.receivedAt))
  return `${sender} · ${location} · ${received}`
}

const NO_SELECTION_VALUE = '__posthaste_no_selection__'
const COMMAND_PANEL_STORAGE_KEY = 'posthaste.commandPalette.panelOffset'

function entryValue(entry: PaletteEntry): string {
  return `${entry.kind}:${entry.id}`
}

function commandIcon(id: PaletteCommandId): React.ReactNode {
  switch (id) {
    case 'compose':
      return (
        <PenSquare
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'reply':
      return (
        <Reply size={15} strokeWidth={1.7} className="text-muted-foreground" />
      )
    case 'archive':
      return (
        <Archive
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'flag':
      return (
        <Tag size={15} strokeWidth={1.7} className="text-muted-foreground" />
      )
    case 'snooze':
      return (
        <Clock3 size={15} strokeWidth={1.7} className="text-muted-foreground" />
      )
    case 'newSmart':
    case 'newRule':
      return (
        <SlidersHorizontal
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'settings':
      return (
        <Settings
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'shortcuts':
      return (
        <Keyboard
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
    case 'account':
      return (
        <UserPlus
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      )
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
  onReply,
  onSelectMessage,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onToggleFlag,
}: CommandPaletteProps) {
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null)
  const serverQuery = normalizeAppliedSearchQuery(query)
  const debouncedServerQuery = useDebouncedValue(serverQuery, 180)
  const { data: sidebar } = useQuery({
    queryKey: ['sidebar'],
    queryFn: fetchSidebar,
  })
  const sourceMessageQueries = useQueries({
    queries: (sidebar?.sources ?? []).map((source) => ({
      queryKey: [
        ...queryKeys.messagesRoot,
        'source-search',
        source.id,
        debouncedServerQuery,
      ] as const,
      queryFn: ({ signal }) =>
        fetchSourceMessages(source.id, null, {
          q: debouncedServerQuery,
          limit: 8,
          signal,
        }),
      enabled: debouncedServerQuery.length > 0,
    })),
  })
  const fetchedSourceMessages = useMemo(
    () => sourceMessageQueries.flatMap((source) => source.data?.items ?? []),
    [sourceMessageQueries],
  )
  const canPreviewSearch =
    debouncedServerQuery.length > 0 &&
    sourceMessageQueries.length > 0 &&
    sourceMessageQueries.every((source) => source.isSuccess)
  const hasPreviewSearchError = sourceMessageQueries.some(
    (source) => source.isError,
  )

  useEffect(() => {
    if (!canPreviewSearch || hasPreviewSearchError) {
      return
    }
    onPreviewSearch(debouncedServerQuery)
  }, [
    canPreviewSearch,
    debouncedServerQuery,
    hasPreviewSearchError,
    onPreviewSearch,
  ])

  const cachedMessages = useMemo(() => {
    const deduped = new Map<string, MessageSummary>()
    for (const message of fetchedSourceMessages) {
      deduped.set(`${message.sourceId}:${message.id}`, message)
    }
    return [...deduped.values()].sort((left, right) =>
      right.receivedAt.localeCompare(left.receivedAt),
    )
  }, [fetchedSourceMessages])

  const results = useMemo(() => {
    const normalized = normalizeQuery(query)

    const commands: PaletteEntry[] = [
      {
        id: 'compose',
        kind: 'command' as const,
        label: 'Compose new message',
        keywords: 'compose new message draft',
        icon: commandIcon('compose'),
        onSelect: onCompose,
      },
      {
        id: 'reply',
        kind: 'command' as const,
        label: 'Reply',
        keywords: 'reply respond answer',
        icon: commandIcon('reply'),
        onSelect: onReply,
      },
      {
        id: 'archive',
        kind: 'command' as const,
        label: 'Archive selected',
        keywords: 'archive selected',
        icon: commandIcon('archive'),
        onSelect: onArchive,
      },
      {
        id: 'flag',
        kind: 'command' as const,
        label: 'Flag message',
        keywords: 'flag star selected',
        icon: commandIcon('flag'),
        onSelect: onToggleFlag,
      },
      {
        id: 'snooze',
        kind: 'command' as const,
        label: 'Snooze…',
        keywords: 'snooze later remind',
        icon: commandIcon('snooze'),
        onSelect: () => onPlaceholderAction('Snooze'),
      },
      {
        id: 'newSmart',
        kind: 'command' as const,
        label: 'New smart mailbox…',
        keywords: 'new smart mailbox create filter',
        icon: commandIcon('newSmart'),
        onSelect: () => onOpenSettings('mailboxes'),
      },
      {
        id: 'newRule',
        kind: 'command' as const,
        label: 'New rule for mailbox…',
        keywords: 'rule mailbox saved search',
        icon: commandIcon('newRule'),
        onSelect: () => onOpenSettings('mailboxes'),
      },
      {
        id: 'settings',
        kind: 'command' as const,
        label: 'Open Settings',
        keywords: 'settings preferences',
        icon: commandIcon('settings'),
        onSelect: () => onOpenSettings(),
      },
      {
        id: 'shortcuts',
        kind: 'command' as const,
        label: 'Keyboard shortcuts',
        keywords: 'keyboard shortcuts help',
        icon: commandIcon('shortcuts'),
        onSelect: onOpenShortcuts,
      },
      {
        id: 'account',
        kind: 'command' as const,
        label: 'Add account…',
        keywords: 'account add source login',
        icon: commandIcon('account'),
        onSelect: () => onOpenSettings('accounts'),
      },
    ].filter(
      (entry) =>
        matchesQuery(normalized, `${entry.label} ${entry.keywords}`) &&
        (hasSelectedMessage ||
          !['archive', 'flag', 'reply'].includes(entry.id)),
    )

    const messages = cachedMessages
      .slice(0, 8)
      .map<PaletteEntry>((message) => ({
        id: `${message.sourceId}:${message.id}`,
        kind: 'message',
        label: message.subject ?? '(no subject)',
        sub: formatMessageSubline(message, sidebar),
        keywords: `${message.subject ?? ''} ${message.preview ?? ''} ${message.fromName ?? ''} ${message.fromEmail ?? ''}`,
        icon: (
          <MessageSquareText
            size={15}
            strokeWidth={1.7}
            className="text-muted-foreground"
          />
        ),
        onSelect: () => {
          const mailbox = resolveMessageMailbox(sidebar, message)
          if (mailbox) {
            onSelectSourceMailbox(
              message.sourceId,
              mailbox.mailbox.id,
              `${mailbox.source.name} / ${mailbox.mailbox.name}`,
            )
          }
          onSelectMessage(message)
        },
      }))

    const contacts = [
      ...new Set(
        cachedMessages
          .map((message) => message.fromName ?? message.fromEmail)
          .filter(Boolean),
      ),
    ]
      .filter((contact): contact is string => Boolean(contact))
      .filter((contact) => matchesQuery(normalized, contact))
      .slice(0, 5)
      .map<PaletteEntry>((contact) => ({
        id: `contact:${contact}`,
        kind: 'contact',
        label: contact,
        keywords: contact,
        icon: (
          <User size={15} strokeWidth={1.7} className="text-muted-foreground" />
        ),
        onSelect: () => onApplySearch(contact),
      }))

    const mailboxes: PaletteEntry[] = []
    if (sidebar) {
      for (const smartMailbox of sidebar.smartMailboxes) {
        if (matchesQuery(normalized, smartMailbox.name)) {
          mailboxes.push({
            id: `smart:${smartMailbox.id}`,
            kind: 'mailbox',
            label: smartMailbox.name,
            sub: 'Smart mailbox',
            keywords: smartMailbox.name,
            icon: renderMailboxRoleIcon(
              null,
              15,
              smartMailboxFallbackIcon(smartMailbox.name),
            ),
            onSelect: () =>
              onSelectSmartMailbox(smartMailbox.id, smartMailbox.name),
          })
        }
      }
      for (const source of sidebar.sources) {
        for (const mailbox of source.mailboxes) {
          const haystack = `${mailbox.name} ${source.name}`
          if (matchesQuery(normalized, haystack)) {
            mailboxes.push({
              id: `${source.id}:${mailbox.id}`,
              kind: 'mailbox',
              label: mailbox.name,
              sub: source.name,
              keywords: haystack,
              icon: renderMailboxRoleIcon(mailbox.role, 15),
              onSelect: () =>
                onSelectSourceMailbox(
                  source.id,
                  mailbox.id,
                  `${source.name} / ${mailbox.name}`,
                ),
            })
          }
        }
      }
    }

    return [
      { label: 'Messages', items: messages },
      { label: 'Commands', items: commands },
      { label: 'Contacts', items: contacts },
      { label: 'Mailboxes', items: mailboxes.slice(0, 6) },
    ].filter((group) => group.items.length > 0)
  }, [
    cachedMessages,
    hasSelectedMessage,
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
    query,
    sidebar,
  ])

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
      ? NO_SELECTION_VALUE
      : entryValue(flatEntries[activeSelectedIndex])

  function handleQueryChange(value: string) {
    setQuery(value)
    setSelectedIndex(null)
  }

  function runEntry(entry: PaletteEntry) {
    entry.onSelect()
    onClose()
  }

  function applyCurrentQuery() {
    const normalized = normalizeAppliedSearchQuery(query)
    if (!normalized) {
      return
    }
    onApplySearch(normalized)
    onClose()
  }

  function handlePaletteKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const isDownKey =
      event.key === 'ArrowDown' ||
      (event.key === 'j' && (activeSelectedIndex !== null || event.ctrlKey))
    const isUpKey =
      event.key === 'ArrowUp' ||
      (event.key === 'k' && (activeSelectedIndex !== null || event.ctrlKey))

    if (event.key === 'Escape') {
      event.preventDefault()
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
          (entry) => entryValue(entry) === value,
        )
        setSelectedIndex(nextIndex === -1 ? null : nextIndex)
      }}
      onKeyDown={handlePaletteKeyDown}
    >
      <FloatingPanel
        panelLabel="command palette"
        storageKey={COMMAND_PANEL_STORAGE_KEY}
        closeIgnoreSelector="[data-command-search-trigger='true']"
        className="max-w-[40rem]"
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
        <CommandList className="ph-scroll px-0 py-1.5">
          <CommandEmpty>No results. Try a different query.</CommandEmpty>
          {flatEntries.length > 0 && (
            <CommandItem
              aria-hidden="true"
              value={NO_SELECTION_VALUE}
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
                  value={entryValue(item)}
                  className="mx-0 px-4 py-2.5 text-foreground data-[selected=true]:bg-[var(--hover-bg)]"
                  onSelect={() => {
                    runEntry(item)
                  }}
                >
                  <span className="flex size-4 shrink-0 items-center justify-center">
                    {item.icon}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{item.label}</span>
                  {'sub' in item && item.sub && (
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
