/**
 * React hook exposing the mail actions used across surfaces (toggle read/flag,
 * set tags, archive, trash, move to inbox, delete permanently).
 *
 * This is a thin adapter over the operation runner ({@link useOperations}):
 * each method builds a {@link MailOperation} and hands it to `run`, which
 * applies the optimistic cache patch, sends the command, and — for moves and
 * deletes — records an undoable history entry. Undo restores a message to the
 * mailbox it was captured in (not a hardcoded Inbox), so the destination is
 * always correct.
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/L1-ui#undo-system
 */
import { useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'
import type {
  KnownMailboxRole,
  MessageDetail,
  MessageSummary,
} from '../api/types'
import {
  MAILBOX_ROLES,
  SYSTEM_KEYWORD_PREFIX,
  SYSTEM_KEYWORDS,
} from '../domainVocabulary'
import { deriveKeywordState, mailKeys, type MailSelection } from '../mailState'
import {
  destroyOp,
  moveToMailboxRoleOp,
  setKeywordsOp,
  type OperationTarget,
} from '../operations'
import { useOperations } from '../operationsContext'
import type { SourceMessageRef } from '../api/types'

/** Message reference augmented with optional keyword fields for optimistic patching. */
type ReadToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>
/** Message reference augmented with optional keyword fields for optimistic patching. */
type FlagToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>

/** Return type of {@link useEmailActions}. */
export type EmailActions = ReturnType<typeof useEmailActions>

function toSourceMessageRef(
  message: SourceMessageRef | MessageSummary | MailSelection,
): SourceMessageRef {
  if ('messageId' in message) {
    return { sourceId: message.sourceId, messageId: message.messageId }
  }
  return { sourceId: message.sourceId, messageId: message.id }
}

function synthesizeKeywords(
  message: Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>,
) {
  if (message.keywords) {
    return message.keywords
  }

  const keywords: string[] = []
  if (message.isRead) {
    keywords.push(SYSTEM_KEYWORDS.Seen)
  }
  if (message.isFlagged) {
    keywords.push(SYSTEM_KEYWORDS.Flagged)
  }
  return keywords
}

function resolveKeywordState(
  queryClient: ReturnType<typeof useQueryClient>,
  message: ReadToggleTarget | FlagToggleTarget | MessageSummary,
) {
  if ('keywords' in message && Array.isArray(message.keywords)) {
    return deriveKeywordState(message.keywords)
  }

  const target = toSourceMessageRef(message)
  const cachedMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (cachedMessage) {
    return deriveKeywordState(cachedMessage.keywords)
  }

  return deriveKeywordState(synthesizeKeywords(message))
}

function normalizeUserTag(tag: string): string | null {
  const normalized = tag.trim().replace(/\s+/g, ' ')
  if (
    !normalized ||
    normalized.startsWith(SYSTEM_KEYWORD_PREFIX) ||
    normalized.includes('/')
  ) {
    return null
  }
  return normalized
}

function userTagsFromKeywords(keywords: string[]): string[] {
  return keywords.filter(
    (keyword) => !keyword.startsWith(SYSTEM_KEYWORD_PREFIX),
  )
}

function uniqueUserTags(tags: string[]): string[] {
  const seen = new Set<string>()
  const unique: string[] = []
  for (const tag of tags) {
    const normalized = normalizeUserTag(tag)
    if (!normalized) {
      continue
    }
    const key = normalized.toLowerCase()
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    unique.push(normalized)
  }
  return unique
}

/**
 * Build an {@link OperationTarget} for a keyword action from a message that
 * already carries its conversation id (list rows and selections both do), so
 * the runner need not re-resolve it.
 */
function keywordTarget(
  message: ReadToggleTarget | FlagToggleTarget | MessageSummary,
): OperationTarget {
  return {
    ...toSourceMessageRef(message),
    conversationId: message.conversationId,
  }
}

/**
 * Provides optimistic email action methods. Keyword changes (`toggleRead`,
 * `markRead`, `toggleFlag`, `setUserTags`) run optimistically and silently;
 * moves (`archive`, `trash`, `moveToInbox`) are optimistic and undoable;
 * `deletePermanently` is optimistic and irreversible.
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/L1-ui#undo-system
 */
export function useEmailActions() {
  const queryClient = useQueryClient()
  const operations = useOperations()

  const moveToRole = useCallback(
    (
      target: SourceMessageRef,
      role: KnownMailboxRole,
      label: string,
      undoLabel?: string,
    ) => {
      // conversationId is left for the runner to resolve once.
      operations.run(moveToMailboxRoleOp(target, role, label, undoLabel))
    },
    [operations],
  )

  const runKeywords = useCallback(
    (
      message: ReadToggleTarget | FlagToggleTarget | MessageSummary,
      delta: { add: string[]; remove: string[] },
    ) => {
      if (delta.add.length === 0 && delta.remove.length === 0) {
        return
      }
      operations.run(setKeywordsOp(keywordTarget(message), delta))
    },
    [operations],
  )

  return {
    toggleRead: (message: ReadToggleTarget | MessageSummary) => {
      const previous = resolveKeywordState(queryClient, message)
      runKeywords(
        message,
        previous.isRead
          ? { add: [], remove: [SYSTEM_KEYWORDS.Seen] }
          : { add: [SYSTEM_KEYWORDS.Seen], remove: [] },
      )
    },
    markRead: (message: ReadToggleTarget | MessageSummary) => {
      const previous = resolveKeywordState(queryClient, message)
      if (previous.isRead) {
        return
      }
      runKeywords(message, { add: [SYSTEM_KEYWORDS.Seen], remove: [] })
    },
    toggleFlag: (message: FlagToggleTarget | MessageSummary) => {
      const previous = resolveKeywordState(queryClient, message)
      runKeywords(
        message,
        previous.isFlagged
          ? { add: [], remove: [SYSTEM_KEYWORDS.Flagged] }
          : { add: [SYSTEM_KEYWORDS.Flagged], remove: [] },
      )
    },
    setUserTags: (
      message: (ReadToggleTarget | MessageSummary) & { keywords?: string[] },
      tags: string[],
    ) => {
      const previous = resolveKeywordState(queryClient, message)
      const previousUserTags = userTagsFromKeywords(previous.keywords)
      const nextUserTags = uniqueUserTags(tags)
      const add = nextUserTags.filter((tag) => !previousUserTags.includes(tag))
      const remove = previousUserTags.filter(
        (tag) => !nextUserTags.includes(tag),
      )
      runKeywords(message, { add, remove })
    },
    archive: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Archive, 'Message archived'),
    trash: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Trash, 'Message trashed'),
    moveToInbox: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Inbox, 'Moved to Inbox'),
    deletePermanently: (target: SourceMessageRef) =>
      // conversationId is left for the runner to resolve once.
      operations.run(destroyOp(target, 'Permanently deleted')),
    clearError: () => {
      operations.clearError()
    },
    errorMessage: operations.errorMessage,
    isPending: operations.isPending,
  }
}
