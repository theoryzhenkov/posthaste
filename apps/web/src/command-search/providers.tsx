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
  UserPlus,
} from 'lucide-react'
import type { ReactNode } from 'react'

import type { MessageSummary } from '@/api/types'
import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'
import { renderMailboxRoleIcon, smartMailboxFallbackIcon } from '@/mailboxRoles'
import { messagePageClient } from '@/messagePageClient'
import { createOperationContext } from '@/observability'
import { getQueryCompletions, getQueryHelpEntries } from '@/queryLanguage'
import { validateSearchQuery } from '@/queryLanguage'
import { normalizeAppliedSearchQuery } from '@/searchQuery'

import { matchEvidence, matchesQuery, textMatch } from './match'
import type {
  CommandActionId,
  CommandPaletteEntry,
  ProviderSearchRequest,
  SearchCandidate,
  SearchProvider,
} from './types'

const MESSAGE_SUBLINE_DATE_FORMATTER = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
})

function commandIcon(id: CommandActionId): ReactNode {
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

function candidateFromEntry(
  provider: SearchProvider,
  entry: CommandPaletteEntry,
  query: string,
  providerRank: number,
): SearchCandidate {
  const match = textMatch(
    query,
    entry.label,
    `${entry.subtitle ?? ''} ${entry.keywords}`,
  )
  return {
    id: `${provider.id}:${entry.id}`,
    providerId: provider.id,
    vertical: provider.vertical,
    entry,
    providerRank,
    match: matchEvidence(query, 'label', match),
    features: {
      matchKind: match.kind,
      matchScore: match.score,
      vertical: provider.vertical,
    },
  }
}

function resolveMessageMailbox(
  sources: MailboxNavigationReadModels['sources'],
  message: MessageSummary,
) {
  const source = sources.find((item) => item.id === message.sourceId)
  if (!source) return null

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

function messageCandidate(
  provider: SearchProvider,
  message: MessageSummary,
  sources: MailboxNavigationReadModels['sources'],
  query: string,
  rank: number,
): SearchCandidate {
  const mailbox = resolveMessageMailbox(sources, message)
  const label = message.subject ?? '(no subject)'
  const keywords = `${message.subject ?? ''} ${message.preview ?? ''} ${message.fromName ?? ''} ${message.fromEmail ?? ''}`
  // A backend-returned message may have matched server-side (sender, body, or a
  // query-language operator) without locally text-matching its subject. When
  // there is no local text match on a non-empty query, record it as an
  // FTS-style match rather than misclassifying by a naive ':' check.
  const localMatch = textMatch(query, label, keywords)
  const match =
    localMatch.kind === 'none' && query.trim()
      ? { kind: 'fts' as const, score: 65 }
      : localMatch
  return {
    id: `${provider.id}:${message.sourceId}:${message.id}`,
    providerId: provider.id,
    vertical: 'message',
    entry: {
      id: `${message.sourceId}:${message.id}`,
      kind: 'message',
      label,
      subtitle: formatMessageSubline(message, sources),
      keywords,
      icon: (
        <MessageSquareText
          size={15}
          strokeWidth={1.7}
          className="text-muted-foreground"
        />
      ),
      action: {
        kind: 'open-message',
        sourceId: message.sourceId,
        messageId: message.id,
        conversationId: message.conversationId,
        mailboxHint: mailbox
          ? {
              mailboxId: mailbox.mailbox.id,
              name: `${mailbox.source.name} / ${mailbox.mailbox.name}`,
            }
          : undefined,
      },
    },
    providerRank: rank,
    match: matchEvidence(query, 'subject', match),
    features: {
      matchKind: match.kind,
      matchScore: match.score,
      receivedAt: message.receivedAt,
    },
  }
}

function shouldSearchMessages(query: string): string | null {
  const normalized = normalizeAppliedSearchQuery(query)
  if (!normalized) return null
  const validation = validateSearchQuery(normalized)
  if (validation.state !== 'valid') return null
  if (normalized.includes(':')) return normalized
  return normalized.length >= 2 ? normalized : null
}

export function createCommandProviders(input: {
  readModels: Pick<
    MailboxNavigationReadModels,
    'smartMailboxes' | 'sources' | 'tags'
  >
  recentMessages: MessageSummary[]
}): SearchProvider[] {
  const commandProvider: SearchProvider = {
    id: 'commands',
    label: 'Commands',
    vertical: 'command',
    async search(req) {
      const commandEntries: CommandPaletteEntry[] = [
        {
          id: 'compose',
          kind: 'command',
          label: 'Compose new message',
          keywords: 'compose new message draft',
          icon: commandIcon('compose'),
          action: { kind: 'command', commandId: 'compose' },
        },
        {
          id: 'reply',
          kind: 'command',
          label: 'Reply',
          keywords: 'reply respond answer',
          icon: commandIcon('reply'),
          action: { kind: 'command', commandId: 'reply' },
        },
        {
          id: 'archive',
          kind: 'command',
          label: 'Archive selected',
          keywords: 'archive selected',
          icon: commandIcon('archive'),
          action: { kind: 'command', commandId: 'archive' },
        },
        {
          id: 'flag',
          kind: 'command',
          label: 'Flag message',
          keywords: 'flag star selected',
          icon: commandIcon('flag'),
          action: { kind: 'command', commandId: 'flag' },
        },
        {
          id: 'snooze',
          kind: 'command',
          label: 'Snooze…',
          keywords: 'snooze later remind',
          icon: commandIcon('snooze'),
          action: { kind: 'noop', label: 'Snooze' },
        },
        {
          id: 'newSmart',
          kind: 'command',
          label: 'New smart mailbox…',
          keywords: 'new smart mailbox create filter',
          icon: commandIcon('newSmart'),
          action: { kind: 'open-settings', category: 'mailboxes' },
        },
        {
          id: 'newRule',
          kind: 'command',
          label: 'New rule for mailbox…',
          keywords: 'rule mailbox saved search',
          icon: commandIcon('newRule'),
          action: { kind: 'open-settings', category: 'mailboxes' },
        },
        {
          id: 'settings',
          kind: 'command',
          label: 'Open Settings',
          keywords: 'settings preferences',
          icon: commandIcon('settings'),
          action: { kind: 'open-settings' },
        },
        {
          id: 'shortcuts',
          kind: 'command',
          label: 'Keyboard shortcuts',
          keywords: 'keyboard shortcuts help',
          icon: commandIcon('shortcuts'),
          action: { kind: 'command', commandId: 'shortcuts' },
        },
        {
          id: 'account',
          kind: 'command',
          label: 'Add account…',
          keywords: 'account add source login',
          icon: commandIcon('account'),
          action: { kind: 'open-settings', category: 'accounts' },
        },
      ]
      const entries = commandEntries.filter(
        (entry) =>
          matchesQuery(req.query, entry.label, entry.keywords) &&
          (req.context.app.hasSelectedMessage ||
            !['archive', 'flag', 'reply'].includes(entry.id)),
      )

      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(commandProvider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }

  const queryCompletionProvider: SearchProvider = {
    id: 'query-completions',
    label: 'Query Language',
    vertical: 'query-completion',
    async search(req) {
      const completions = getQueryCompletions(req.query, {
        messages: [],
        sources: input.readModels.sources,
        tags: input.readModels.tags,
      }).map<CommandPaletteEntry>((completion) => ({
        id: completion.id,
        kind: 'query-completion',
        label: completion.label,
        subtitle: completion.detail,
        keywords: `${completion.label} ${completion.detail}`,
        icon: (
          <ListFilter
            size={15}
            strokeWidth={1.7}
            className="text-muted-foreground"
          />
        ),
        action: { kind: 'replace-query', query: completion.replacement },
        closeOnSelect: false,
      }))
      const help = getQueryHelpEntries(req.query).map<CommandPaletteEntry>(
        (entry) => ({
          id: entry.id,
          kind: 'query-completion',
          label: entry.label,
          subtitle: entry.detail,
          keywords: entry.keywords,
          icon: (
            <CircleHelp
              size={15}
              strokeWidth={1.7}
              className="text-muted-foreground"
            />
          ),
          action: { kind: 'replace-query', query: entry.replacement },
          closeOnSelect: false,
        }),
      )
      const entries = [...completions, ...help]
      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(
              queryCompletionProvider,
              entry,
              req.query,
              index,
            ),
          ),
        nextCursor: null,
      }
    },
  }

  const mailboxProvider: SearchProvider = {
    id: 'mailboxes',
    label: 'Mailboxes',
    vertical: 'mailbox',
    async search(req) {
      const entries: CommandPaletteEntry[] = []
      for (const smartMailbox of input.readModels.smartMailboxes) {
        if (!matchesQuery(req.query, smartMailbox.name)) continue
        entries.push({
          id: `smart:${smartMailbox.id}`,
          kind: 'mailbox',
          label: smartMailbox.name,
          subtitle: 'Smart mailbox',
          keywords: smartMailbox.name,
          icon: renderMailboxRoleIcon(
            null,
            15,
            smartMailboxFallbackIcon(smartMailbox.name),
          ),
          action: {
            kind: 'open-smart-mailbox',
            smartMailboxId: smartMailbox.id,
            name: smartMailbox.name,
          },
        })
      }
      for (const source of input.readModels.sources) {
        for (const mailbox of source.mailboxes) {
          const haystack = `${mailbox.name} ${source.name} ${mailbox.role ?? ''}`
          if (!matchesQuery(req.query, mailbox.name, haystack)) continue
          entries.push({
            id: `${source.id}:${mailbox.id}`,
            kind: 'mailbox',
            label: mailbox.name,
            subtitle: source.name,
            keywords: haystack,
            icon: renderMailboxRoleIcon(mailbox.role, 15),
            action: {
              kind: 'open-source-mailbox',
              sourceId: source.id,
              mailboxId: mailbox.id,
              name: `${source.name} / ${mailbox.name}`,
            },
          })
        }
      }
      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(mailboxProvider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }

  const tagProvider: SearchProvider = {
    id: 'tags',
    label: 'Tags',
    vertical: 'tag',
    async search(req) {
      const entries = input.readModels.tags
        .filter((tag) => matchesQuery(req.query, tag.name))
        .map<CommandPaletteEntry>((tag) => ({
          id: tag.name,
          kind: 'tag',
          label: tag.name,
          subtitle: `${tag.totalMessages} messages`,
          keywords: tag.name,
          icon: (
            <Tag
              size={15}
              strokeWidth={1.7}
              className="text-muted-foreground"
            />
          ),
          action: { kind: 'apply-query', query: `tag:${tag.name}` },
        }))
      return {
        candidates: entries
          .slice(0, req.limit)
          .map((entry, index) =>
            candidateFromEntry(tagProvider, entry, req.query, index),
          ),
        nextCursor: null,
      }
    },
  }

  const messageProvider: SearchProvider = {
    id: 'messages',
    label: 'Messages',
    vertical: 'message',
    remote: true,
    async search(req: ProviderSearchRequest) {
      if (!req.query.trim()) {
        return {
          candidates: input.recentMessages
            .slice(0, req.limit)
            .map((message, index) =>
              messageCandidate(
                messageProvider,
                message,
                input.readModels.sources,
                req.query,
                index,
              ),
            ),
          nextCursor: null,
        }
      }

      const serverQuery = shouldSearchMessages(req.query)
      if (!serverQuery) {
        return { candidates: [], nextCursor: null }
      }

      const page = await messagePageClient.fetchPage({
        scope: { kind: 'global' },
        query: serverQuery,
        cursor: req.cursor,
        limit: req.limit,
        sort: 'date',
        sortDir: 'desc',
        signal: req.signal,
        operation: createOperationContext(
          'mail.search.preview',
          'command-palette',
        ),
      })
      return {
        candidates: page.items.map((message, index) =>
          messageCandidate(
            messageProvider,
            message,
            input.readModels.sources,
            req.query,
            index,
          ),
        ),
        nextCursor: page.nextCursor,
      }
    },
  }

  return [
    commandProvider,
    queryCompletionProvider,
    mailboxProvider,
    tagProvider,
    messageProvider,
  ]
}
