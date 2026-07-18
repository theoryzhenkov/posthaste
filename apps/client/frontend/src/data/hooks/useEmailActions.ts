/**
 * React hook exposing the mail actions used across surfaces (toggle read/flag,
 * set tags, archive, trash, move to inbox, snooze, delete permanently).
 *
 * Each method posts one typed command through the data layer's verb set. The
 * renderer keeps no optimistic overlay or undo history of its own: acceptance
 * invalidates every query, the answers catch up to the new generation, and
 * undo is the backend rev-log (a move toast's "Undo" reverses the latest
 * reversible action via the supplied {@link undo} callback).
 */
import { useQueryClient } from '@tanstack/react-query'
import { useCallback, useRef, useState } from 'react'
import { toast } from 'sonner'
import type { MessageDetailResult, MessageSummary } from '@/gen'
import {
  MAILBOX_ROLES,
  SYSTEM_KEYWORD_PREFIX,
  SYSTEM_KEYWORDS,
  type KnownMailboxRole,
} from '../../domain/vocabulary'
import { queryKeys, useCommands, type MailSelection } from '@/data'

/** A message reference: the account (source) it lives in plus its id. */
export interface SourceMessageRef {
  sourceId: string
  messageId: string
}

/** Message reference augmented with optional keyword fields for state derivation. */
type ReadToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>
/** Message reference augmented with optional keyword fields for state derivation. */
type FlagToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>

/** Return type of {@link useEmailActions}. */
export type EmailActions = ReturnType<typeof useEmailActions>

/**
 * Client-side grace before a discarded draft's delete-draft command is
 * actually dispatched. During this window a "Draft discarded" toast offers
 * Undo, which cancels the pending dispatch so nothing is ever sent —
 * replacing the Trash safety net a normal message relies on. The row
 * disappears when the command lands and the queries re-answer.
 */
export const DRAFT_DISCARD_GRACE_MS = 5000

interface KeywordState {
  isRead: boolean
  isFlagged: boolean
  keywords: string[]
}

function deriveKeywordState(keywords: string[]): KeywordState {
  return {
    isRead: keywords.includes(SYSTEM_KEYWORDS.Seen),
    isFlagged: keywords.includes(SYSTEM_KEYWORDS.Flagged),
    keywords,
  }
}

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
): KeywordState {
  if ('keywords' in message && Array.isArray(message.keywords)) {
    return deriveKeywordState(message.keywords)
  }

  // Fall back to the mirrored messageDetail answer, when one is cached.
  const target = toSourceMessageRef(message)
  const cachedDetail = queryClient.getQueryData<MessageDetailResult>(
    queryKeys.messageDetail({
      accountId: target.sourceId,
      messageId: target.messageId,
    }),
  )
  if (cachedDetail) {
    return deriveKeywordState(cachedDetail.summary.keywords)
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
 * Provides email action methods backed by typed commands. Keyword changes
 * (`toggleRead`, `markRead`, `toggleFlag`, `setUserTags`) run silently; moves
 * (`archive`, `trash`, `moveToInbox`) toast with an Undo that calls the
 * supplied {@link undo} callback; `deletePermanently` is irreversible (no Undo).
 */
export function useEmailActions({ undo }: { undo: () => void }) {
  const queryClient = useQueryClient()
  const commands = useCommands()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const pendingRef = useRef(0)
  const [isPending, setIsPending] = useState(false)
  /** Pending draft-discard timers keyed by `sourceId:messageId`; an Undo clears
   * the entry before the delay elapses so the delete-draft command never
   * dispatches. */
  const discardTimersRef = useRef(
    new Map<string, ReturnType<typeof setTimeout>>(),
  )

  const setPending = useCallback((delta: number) => {
    pendingRef.current = Math.max(0, pendingRef.current + delta)
    setIsPending(pendingRef.current > 0)
  }, [])

  const dispatch = useCallback(
    (input: {
      run: () => Promise<unknown>
      /** Toast copy shown on success; omitted for silent keyword changes. */
      label?: string
      /** When set, the toast offers Undo via the backend rev-log. */
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
                    onClick: () => undo(),
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
    [setPending, undo],
  )

  const moveToRole = useCallback(
    (target: SourceMessageRef, role: KnownMailboxRole, label: string) => {
      dispatch({
        label,
        run: () => {
          switch (role) {
            case MAILBOX_ROLES.Archive:
              return commands.archive(target.sourceId, target.messageId)
            case MAILBOX_ROLES.Trash:
              return commands.trash(target.sourceId, target.messageId)
            default:
              return commands.moveToRole(target.sourceId, target.messageId, role)
          }
        },
        undoSourceId: target.sourceId,
      })
    },
    [commands, dispatch],
  )

  /**
   * Discard a draft. Never routes to the trash command. The "Draft discarded"
   * toast appears immediately, but the `discardDraft` command dispatch is
   * deferred by {@link DRAFT_DISCARD_GRACE_MS}: Undo cancels the pending
   * timer, so nothing is ever sent (the SAFE direction). Once the grace
   * elapses the command posts; the row disappears when the answer catches up.
   * Tab close during the grace drops the timer with the page, so the draft is
   * kept — we deliberately do NOT force-flush on unload.
   */
  const discardDraft = useCallback(
    (target: SourceMessageRef & { draftId?: string | null }) => {
      setErrorMessage(null)
      const key = `${target.sourceId}:${target.messageId}`
      // Coalesce repeated discards of the same draft into one pending dispatch.
      if (discardTimersRef.current.has(key)) {
        return
      }
      // The stable draft id resolves the live draft across provider id
      // rotation; fall back to the row's messageId for a row without one.
      const draftId = target.draftId ?? target.messageId
      const timer = setTimeout(() => {
        discardTimersRef.current.delete(key)
        setPending(1)
        void commands
          .discardDraft(target.sourceId, draftId)
          .catch((error: unknown) => {
            setErrorMessage(
              error instanceof Error ? error.message : 'Operation failed',
            )
          })
          .finally(() => setPending(-1))
      }, DRAFT_DISCARD_GRACE_MS)
      discardTimersRef.current.set(key, timer)
      toast('Draft discarded', {
        action: {
          label: 'Undo',
          onClick: () => {
            const pending = discardTimersRef.current.get(key)
            if (pending) {
              clearTimeout(pending)
              discardTimersRef.current.delete(key)
            }
          },
        },
        duration: DRAFT_DISCARD_GRACE_MS,
      })
    },
    [commands, setPending],
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
          commands.setKeywords(
            target.sourceId,
            target.messageId,
            delta.add,
            delta.remove,
          ),
      })
    },
    [commands, dispatch],
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
    discardDraft,
    moveToInbox: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Inbox, 'Moved to Inbox'),
    /** Move to an EXPLICIT mailbox (the parameterized "Move to…" action) via
     *  the typed `replaceMailboxes` command. Undoable like any structural
     *  move. */
    moveToMailbox: (
      target: SourceMessageRef,
      mailboxId: string,
      mailboxName: string,
    ) =>
      dispatch({
        label: `Moved to ${mailboxName}`,
        run: () => commands.move(target.sourceId, target.messageId, [mailboxId]),
        undoSourceId: target.sourceId,
      }),
    /** Snooze until the given instant (epoch milliseconds). */
    snooze: (target: SourceMessageRef, until: number) =>
      dispatch({
        label: 'Message snoozed',
        run: () =>
          commands.snooze(
            target.sourceId,
            target.messageId,
            new Date(until).toISOString(),
          ),
        undoSourceId: target.sourceId,
      }),
    unsnooze: (target: SourceMessageRef) =>
      dispatch({
        label: 'Message unsnoozed',
        run: () => commands.unsnooze(target.sourceId, target.messageId),
        undoSourceId: target.sourceId,
      }),
    deletePermanently: (target: SourceMessageRef) =>
      dispatch({
        label: 'Permanently deleted',
        run: () => commands.destroy(target.sourceId, target.messageId),
      }),
    /**
     * RFC 8058 one-click unsubscribe: the BACKEND performs the POST to the
     * list server (https-only, credential-free); this just triggers it and
     * reports the acceptance. Callers must have confirmed with the user first
     * (the action's `confirm` gate).
     */
    unsubscribe: (target: SourceMessageRef) => {
      setErrorMessage(null)
      setPending(1)
      return commands
        .run({
          unsubscribe: {
            accountId: target.sourceId,
            messageId: target.messageId,
          },
        })
        .then(() => {
          toast('Unsubscribe request sent', { duration: 5000 })
        })
        .catch((error: unknown) => {
          toast(
            `Unsubscribe failed: ${
              error instanceof Error ? error.message : 'request failed'
            }`,
            { duration: 8000 },
          )
        })
        .finally(() => setPending(-1))
    },
    clearError: () => setErrorMessage(null),
    errorMessage,
    isPending,
  }
}
