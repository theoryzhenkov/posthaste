/**
 * React hook exposing the mail actions used across surfaces (toggle read/flag,
 * set tags, archive, trash, move to inbox, delete permanently).
 *
 * Each method submits a runtime named mutation directly. The renderer keeps no
 * optimistic overlay or undo history of its own: the runtime applies the
 * mutation, the optimistic/authoritative state flows back through view frames
 * and domain-event cache updates, and undo is the runtime-owned
 * `message.applyDiff` stack (so a move toast's "Undo" reverses the latest
 * reversible action via the supplied {@link undo} callback).
 *
 * @spec docs/L1-ui#data-fetching
 * @spec docs/runtime/mutations/L1#mutation-pipeline-and-catalog
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

/** Message reference augmented with optional keyword fields for state derivation. */
type ReadToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>
/** Message reference augmented with optional keyword fields for state derivation. */
type FlagToggleTarget = MailSelection &
  Partial<Pick<MessageSummary, 'isFlagged' | 'isRead' | 'keywords'>>

/** Return type of {@link useEmailActions}. */
export type EmailActions = ReturnType<typeof useEmailActions>

/**
 * Client-side grace before a discarded draft's delete-draft op is actually
 * dispatched (D127). During this window a "Draft discarded" toast offers Undo,
 * which cancels the pending dispatch so nothing is ever sent — replacing the
 * Trash safety net a normal message relies on.
 */
export const DRAFT_DISCARD_GRACE_MS = 5000

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
 * Provides email action methods backed by runtime named mutations. Keyword
 * changes (`toggleRead`, `markRead`, `toggleFlag`, `setUserTags`) run silently;
 * moves (`archive`, `trash`, `moveToInbox`) toast with an Undo that calls the
 * supplied {@link undo} callback; `deletePermanently` is irreversible (no Undo).
 *
 * @spec docs/L1-ui#data-fetching
 */
export function useEmailActions({ undo }: { undo: () => void }) {
  const queryClient = useQueryClient()
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const pendingRef = useRef(0)
  const [isPending, setIsPending] = useState(false)
  /** Pending draft-discard timers keyed by `sourceId:messageId`; an Undo clears
   * the entry before the delay elapses so the delete-draft op never dispatches. */
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
        run: () =>
          runtimeMutations.messages.moveToMailboxRole(
            {
              messageId: target.messageId,
              role,
              sourceId: target.sourceId,
            },
            // A structural user action (archive/trash/move) is undoable.
            { userInitiated: true },
          ),
        undoSourceId: target.sourceId,
      })
    },
    [dispatch],
  )

  /**
   * D130/D127 — discard a draft. Never routes to the trash mutation. The
   * removal now flows through the optimistic runtime-mutation path
   * ({@link runtimeMutations.messages.discardDraft}): the destroy folds
   * instantly on the row's `messageId` (the blink), settles on the runtime
   * notification, and reverts + surfaces the error on failure — replacing the
   * old fire-and-forget POST whose deferred dispatch never reliably landed
   * ("discard does nothing"). The stable `draftId` (D131) rides along so the
   * far node resolves the current live Email even after a JMAP autosave
   * rotates the id.
   *
   * Undo shape (D134): a short {@link DRAFT_DISCARD_GRACE_MS} pre-commit grace
   * gates the dispatch — the "Undo" toast cancels the pending timer so nothing
   * is ever dispatched and the draft survives (the SAFE direction). Once the
   * grace elapses the optimistic destroy is dispatched and is itself
   * reversible via the settlement (a rejected destroy reverts the fold). Tab
   * close during the grace drops the timer with the page, so the draft is
   * kept. We deliberately do NOT force-flush on unload.
   */
  const discardDraft = useCallback(
    (target: SourceMessageRef & { draftId?: string | null }) => {
      setErrorMessage(null)
      const key = `${target.sourceId}:${target.messageId}`
      // Coalesce repeated discards of the same draft into one pending dispatch.
      if (discardTimersRef.current.has(key)) {
        return
      }
      // The stable draft id (D131) resolves the live Email across id rotation;
      // fall back to the row's messageId for a legacy row without one.
      const draftId = target.draftId ?? target.messageId
      const timer = setTimeout(() => {
        discardTimersRef.current.delete(key)
        setPending(1)
        void runtimeMutations.messages
          .discardDraft(
            {
              sourceId: target.sourceId,
              messageId: target.messageId,
              draftId,
            },
            { userInitiated: true },
          )
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
    [setPending],
  )

  const runKeywords = useCallback(
    (
      message: ReadToggleTarget | FlagToggleTarget | MessageSummary,
      delta: { add: string[]; remove: string[] },
      options?: { userInitiated?: boolean },
    ) => {
      if (delta.add.length === 0 && delta.remove.length === 0) {
        return
      }
      const target = toSourceMessageRef(message)
      dispatch({
        run: () =>
          runtimeMutations.messages.command(
            {
              command: {
                kind: 'setKeywords',
                add: delta.add,
                remove: delta.remove,
              } satisfies MessageCommand,
              messageId: target.messageId,
              sourceId: target.sourceId,
            },
            options,
          ),
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
        // An explicit read toggle is a user action (undoable).
        { userInitiated: true },
      )
    },
    markRead: (message: ReadToggleTarget | MessageSummary) => {
      const previous = resolveKeywordState(queryClient, message)
      if (previous.isRead) {
        return
      }
      // Auto-mark-read (useAutoMarkRead) is a side-effect, not a user gesture —
      // NOT tagged userInitiated, so it doesn't pollute the undo history.
      runKeywords(message, { add: [SYSTEM_KEYWORDS.Seen], remove: [] })
    },
    toggleFlag: (message: FlagToggleTarget | MessageSummary) => {
      const previous = resolveKeywordState(queryClient, message)
      runKeywords(
        message,
        previous.isFlagged
          ? { add: [], remove: [SYSTEM_KEYWORDS.Flagged] }
          : { add: [SYSTEM_KEYWORDS.Flagged], remove: [] },
        { userInitiated: true },
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
      runKeywords(message, { add, remove }, { userInitiated: true })
    },
    archive: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Archive, 'Message archived'),
    trash: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Trash, 'Message trashed'),
    discardDraft,
    moveToInbox: (target: SourceMessageRef) =>
      moveToRole(target, MAILBOX_ROLES.Inbox, 'Moved to Inbox'),
    snooze: (target: SourceMessageRef, until: number) =>
      dispatch({
        label: 'Message snoozed',
        run: () =>
          runtimeMutations.messages.snooze(
            { messageId: target.messageId, sourceId: target.sourceId, until },
            // A user snooze is a structural move (undoable via the RevLog).
            { userInitiated: true },
          ),
        undoSourceId: target.sourceId,
      }),
    unsnooze: (target: SourceMessageRef) =>
      dispatch({
        label: 'Message unsnoozed',
        run: () =>
          runtimeMutations.messages.unsnooze(
            { messageId: target.messageId, sourceId: target.sourceId },
            { userInitiated: true },
          ),
        undoSourceId: target.sourceId,
      }),
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
