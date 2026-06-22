/**
 * React hook exposing the mail actions used across surfaces (toggle read/flag,
 * set tags, archive, trash, move to inbox, delete permanently).
 *
 * Each method submits a runtime named mutation directly. The renderer keeps no
 * optimistic overlay or undo history of its own: the runtime applies the
 * mutation, the optimistic/authoritative state flows back through view frames
 * and domain-event cache updates, and undo is the runtime-owned `mutation.undo`
 * stack (so a move toast's "Undo" reverses the last action).
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/runtime/L2#mutation-pipeline-and-catalog
 */
import { useQueryClient } from '@tanstack/react-query'
import { useCallback, useRef, useState } from 'react'
import { toast } from 'sonner'
import type {
  KnownMailboxRole,
  MessageCommand,
  MessageDetail,
  MessageSummary,
  SourceMessageRef,
} from '../api/types'
import {
  MAILBOX_ROLES,
  SYSTEM_KEYWORD_PREFIX,
  SYSTEM_KEYWORDS,
} from '../domainVocabulary'
import { deriveKeywordState, mailKeys, type MailSelection } from '../mailState'
import { runtimeMutations } from '../runtime/mutations'
import { runtimeSessionClient } from '../runtime/sessionClient'

/** Message reference augmented with optional keyword fields for state derivation. */
type ReadToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>
/** Message reference augmented with optional keyword fields for state derivation. */
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

/** Reverse the most recent action through the runtime's undo stack. */
function undoLastRuntimeMutation(sourceId: string) {
  void runtimeSessionClient
    .runMutation({ name: 'mutation.undo', args: {}, sourceId })
    .catch(() => {})
}

/**
 * Provides email action methods backed by runtime named mutations. Keyword
 * changes (`toggleRead`, `markRead`, `toggleFlag`, `setUserTags`) run silently;
 * moves (`archive`, `trash`, `moveToInbox`) toast with an Undo that calls the
 * runtime undo; `deletePermanently` is irreversible (no Undo).
 *
 * @spec docs/L1-ui#data-fetching
 */
export function useEmailActions() {
  const queryClient = useQueryClient()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const pendingRef = useRef(0)
  const [isPending, setIsPending] = useState(false)

  const setPending = useCallback((delta: number) => {
    pendingRef.current = Math.max(0, pendingRef.current + delta)
    setIsPending(pendingRef.current > 0)
  }, [])

  const dispatch = useCallback(
    (input: {
      run: () => Promise<unknown>
      /** Toast copy shown on success; omitted for silent keyword changes. */
      label?: string
      /** When set, the toast offers Undo via the runtime undo stack. */
      undoSourceId?: string
    }) => {
      setErrorMessage(null)
      setPending(1)
      void input
        .run()
        .then(() => {
          if (!input.label) {
            return
          }
          toast(
            input.label,
            input.undoSourceId
              ? {
                  action: {
                    label: 'Undo',
                    onClick: () => undoLastRuntimeMutation(input.undoSourceId!),
                  },
                  duration: 5000,
                }
              : { duration: 5000 },
          )
        })
        .catch((error: unknown) => {
          setErrorMessage(
            error instanceof Error ? error.message : 'Operation failed',
          )
        })
        .finally(() => setPending(-1))
    },
    [setPending],
  )

  const moveToRole = useCallback(
    (target: SourceMessageRef, role: KnownMailboxRole, label: string) => {
      dispatch({
        label,
        run: () =>
          runtimeMutations.messages.moveToMailboxRole({
            messageId: target.messageId,
            role,
            sourceId: target.sourceId,
          }),
        undoSourceId: target.sourceId,
      })
    },
    [dispatch],
  )

  const runKeywords = useCallback(
    (
      message: ReadToggleTarget | FlagToggleTarget | MessageSummary,
      delta: { add: string[]; remove: string[] },
    ) => {
      if (delta.add.length === 0 && delta.remove.length === 0) {
        return
      }
      const target = toSourceMessageRef(message)
      dispatch({
        run: () =>
          runtimeMutations.messages.command({
            command: {
              kind: 'setKeywords',
              add: delta.add,
              remove: delta.remove,
            } satisfies MessageCommand,
            messageId: target.messageId,
            sourceId: target.sourceId,
          }),
      })
    },
    [dispatch],
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
      dispatch({
        label: 'Permanently deleted',
        run: () =>
          runtimeMutations.messages.command({
            command: { kind: 'destroy' } satisfies MessageCommand,
            messageId: target.messageId,
            sourceId: target.sourceId,
          }),
      }),
    clearError: () => setErrorMessage(null),
    errorMessage,
    isPending,
  }
}
