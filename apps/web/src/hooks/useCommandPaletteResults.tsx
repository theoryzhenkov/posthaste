import {
  Archive,
  CircleHelp,
  Clock3,
  Keyboard,
  ListFilter,
  MessageSquareText,
  PenSquare,
  Reply,
  Settings,
  SlidersHorizontal,
  Tag,
  User,
  UserPlus,
} from 'lucide-react'
import { useMemo, type ReactNode } from 'react'

import type { MessageSummary } from '@/api/types'
import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'
import { renderMailboxRoleIcon, smartMailboxFallbackIcon } from '@/mailboxRoles'
import { getQueryCompletions, getQueryHelpEntries } from '@/queryLanguage'
import type { SettingsSurfaceCategory } from '@/surfaces'

export type SettingsCategory = SettingsSurfaceCategory

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

export type CommandPaletteEntry = {
  id: string
  kind: 'command' | 'message' | 'contact' | 'mailbox' | 'query'
  label: string
  sub?: string
  keywords: string
  icon: ReactNode
  closeOnSelect?: boolean
  onSelect: () => void
}

export interface CommandPaletteResultGroup {
  label: string
  items: CommandPaletteEntry[]
}

interface UseCommandPaletteResultsArgs {
  cachedMessages: MessageSummary[]
  hasSelectedMessage: boolean
  onApplySearch: (query: string) => void
  onArchive: () => void
  onCompose: () => void
  onOpenSettings: (category?: SettingsCategory) => void
  onOpenShortcuts: () => void
  onPlaceholderAction: (label: string) => void
  onReplaceQuery: (query: string) => void
  onReply: () => void
  onSelectMessage: (message: MessageSummary) => void
  onSelectSmartMailbox: (smartMailboxId: string, name: string) => void
  onSelectSourceMailbox: (
    sourceId: string,
    mailboxId: string,
    name: string,
  ) => void
  onToggleFlag: () => void
  query: string
  readModels: Pick<
    MailboxNavigationReadModels,
    'smartMailboxes' | 'sources' | 'tags'
  >
}

export const NO_COMMAND_PALETTE_SELECTION = '__posthaste_no_selection__'
const MESSAGE_SUBLINE_DATE_FORMATTER = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
})

export function commandPaletteEntryValue(entry: CommandPaletteEntry): string {
  return `${entry.kind}:${entry.id}`
}

function normalizeQuery(value: string): string {
  return value.trim().toLowerCase()
}

function matchesQuery(query: string, text: string): boolean {
  return query.length === 0 || text.toLowerCase().includes(query)
}

function resolveMessageMailbox(
  sources: MailboxNavigationReadModels['sources'],
  message: MessageSummary,
) {
  const source = sources.find((item) => item.id === message.sourceId)
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
  sources: MailboxNavigationReadModels['sources'],
): string {
  const sender = message.fromName ?? message.fromEmail ?? 'Unknown'
  const mailbox = resolveMessageMailbox(sources, message)
  const location = mailbox
    ? `${mailbox.source.name} / ${mailbox.mailbox.name}`
    : message.sourceName
  const received = MESSAGE_SUBLINE_DATE_FORMATTER.format(
    new Date(message.receivedAt),
  )
  return `${sender} · ${location} · ${received}`
}

function commandIcon(id: PaletteCommandId): ReactNode {
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

export function useCommandPaletteResults({
  cachedMessages,
  hasSelectedMessage,
  onApplySearch,
  onArchive,
  onCompose,
  onOpenSettings,
  onOpenShortcuts,
  onPlaceholderAction,
  onReplaceQuery,
  onReply,
  onSelectMessage,
  onSelectSmartMailbox,
  onSelectSourceMailbox,
  onToggleFlag,
  query,
  readModels,
}: UseCommandPaletteResultsArgs): CommandPaletteResultGroup[] {
  return useMemo(() => {
    const normalized = normalizeQuery(query)

    const queryCompletions = getQueryCompletions(query, {
      messages: cachedMessages,
      sources: readModels.sources,
      tags: readModels.tags,
    }).map<CommandPaletteEntry>((completion) => ({
      id: completion.id,
      kind: 'query',
      label: completion.label,
      sub: completion.detail,
      keywords: `${completion.label} ${completion.detail}`,
      icon: (
        <ListFilter
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      ),
      closeOnSelect: false,
      onSelect: () => onReplaceQuery(completion.replacement),
    }))

    const queryHelp = getQueryHelpEntries(query).map<CommandPaletteEntry>(
      (entry) => ({
        id: entry.id,
        kind: 'query',
        label: entry.label,
        sub: entry.detail,
        keywords: entry.keywords,
        icon: (
          <CircleHelp
            size={15}
            strokeWidth={1.7}
            className="text-muted-foreground"
          />
        ),
        closeOnSelect: false,
        onSelect: () => onReplaceQuery(entry.replacement),
      }),
    )

    const commands: CommandPaletteEntry[] = [
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
      .map<CommandPaletteEntry>((message) => ({
        id: `${message.sourceId}:${message.id}`,
        kind: 'message',
        label: message.subject ?? '(no subject)',
        sub: formatMessageSubline(message, readModels.sources),
        keywords: `${message.subject ?? ''} ${message.preview ?? ''} ${message.fromName ?? ''} ${message.fromEmail ?? ''}`,
        icon: (
          <MessageSquareText
            size={15}
            strokeWidth={1.7}
            className="text-muted-foreground"
          />
        ),
        onSelect: () => {
          const mailbox = resolveMessageMailbox(readModels.sources, message)
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
      .map<CommandPaletteEntry>((contact) => ({
        id: `contact:${contact}`,
        kind: 'contact',
        label: contact,
        keywords: contact,
        icon: (
          <User size={15} strokeWidth={1.7} className="text-muted-foreground" />
        ),
        onSelect: () => onApplySearch(contact),
      }))

    const mailboxes: CommandPaletteEntry[] = []
    for (const smartMailbox of readModels.smartMailboxes) {
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
    for (const source of readModels.sources) {
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

    return [
      { label: 'Suggestions', items: queryCompletions },
      { label: 'Messages', items: messages },
      { label: 'Query Language', items: queryHelp },
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
    onReplaceQuery,
    onReply,
    onSelectMessage,
    onSelectSmartMailbox,
    onSelectSourceMailbox,
    onToggleFlag,
    query,
    readModels,
  ])
}
