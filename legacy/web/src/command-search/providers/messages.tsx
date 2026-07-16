import { MessageSquareText } from 'lucide-react'

import type { MessageSummary } from '@/api/types'
import type { MailboxNavigationReadModels } from '@/mailboxNavigationReadModels'
import { messagePageClient } from '@/messagePageClient'
import { createOperationContext } from '@/observability'
import { validateSearchQuery } from '@/queryLanguage'
import { normalizeAppliedSearchQuery } from '@/searchQuery'

import { matchEvidence, textMatch } from '../match'
import type {
  CommandPaletteEntry,
  ProviderSearchRequest,
  SearchCandidate,
  SearchProvider,
} from '../types'

const MESSAGE_SUBLINE_DATE_FORMATTER = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
})

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
    } satisfies CommandPaletteEntry,
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

export function createMessageProvider(input: {
  readModels: Pick<MailboxNavigationReadModels, 'sources'>
  recentMessages: MessageSummary[]
}): SearchProvider {
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
  return messageProvider
}
