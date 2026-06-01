/**
 * Client-side React Query cache helpers for conversations, messages, and keyword mutations.
 *
 * This module owns the cache key schema, optimistic update logic,
 * local-echo suppression, and conversation-summary derivation.
 *
 * @spec docs/L1-ui#data-fetching
 */
import type { InfiniteData, QueryClient, QueryKey } from '@tanstack/react-query'
import type {
  ConversationPage,
  ConversationSummary,
  ConversationView,
  DomainEvent,
  MessageCommand,
  MessageDetail,
  MessagePage,
  MessageSummary,
  SourceMessageRef,
} from './api/types'
import { queryKeys } from './queryKeys'

/**
 * Selected message reference used by list and detail views.
 * @spec docs/L1-ui#messagelist
 */
export type MailSelection = SourceMessageRef & { conversationId: string }

/**
 * Current sidebar selection -- either a smart mailbox or a source+mailbox pair.
 * @spec docs/L0-ui#navigation-model
 */
export type MailViewSelection =
  | { kind: 'smart-mailbox'; id: string }
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
  | null

/**
 * Normalized conversation page stored in the infinite query cache.
 * Summaries are extracted into per-ID cache entries; only IDs remain here.
 * @spec docs/L1-api#cursor-pagination
 */
export type ConversationPageSlice = {
  itemIds: string[]
  nextCursor: string | null
}

/**
 * Canonical React Query key builders for mail-related data.
 * @spec docs/L1-ui#data-fetching
 */
export const mailKeys = {
  messageRoot: queryKeys.messageDetailsRoot,
  conversationRoot: queryKeys.conversationDetailsRoot,
  conversationSummaryRoot: queryKeys.conversationSummariesRoot,
  message: (sourceId: string, messageId: string) =>
    ['message', sourceId, messageId] as const,
  conversation: (conversationId: string) =>
    ['conversation', conversationId] as const,
  conversationSummary: (conversationId: string) =>
    ['conversation-summary', conversationId] as const,
  view: (
    selection: MailViewSelection,
    sort?: { columnId: string; direction: string },
    q?: string,
  ) => {
    const base = !selection
      ? (['conversations', 'none'] as const)
      : selection.kind === 'smart-mailbox'
        ? (['conversations', 'smart-mailbox', selection.id] as const)
        : ([
            'conversations',
            'source-mailbox',
            selection.sourceId,
            selection.mailboxId,
          ] as const)
    const withSort = sort ? [...base, sort.columnId, sort.direction] : base
    if (q) {
      return [...withSort, 'q', q] as const
    }
    return withSort
  },
}

/** Snapshot of a single query entry for optimistic rollback. */
export type QuerySnapshot = {
  data: unknown
  existed: boolean
  queryKey: QueryKey
}

/** Derived boolean flags from raw JMAP keyword strings. */
export type KeywordState = Pick<
  MessageSummary,
  'isFlagged' | 'isRead' | 'keywords'
>

/** Before/after pair for an optimistic keyword mutation. */
export type KeywordPatch = {
  next: KeywordState
  previous: KeywordState
}

/**
 * The full mutable JMAP state of a message that any mail operation can change
 * and later restore. Capturing this before-image is what lets undo be a single
 * generic "restore to previous state" primitive instead of per-operation logic.
 * @spec docs/L1-ui#undo-system
 */
export type MutableState = {
  mailboxIds: string[]
  keywords: string[]
}

type ReconcileOptions = {
  allowHeuristicFlagClear?: boolean
}

/** Result of applying an optimistic keyword patch to the cache. */
export type CachePatchResult = {
  incomplete: boolean
  snapshots: QuerySnapshot[]
}

const LOCAL_MUTATION_TTL_MS = 5_000
const localMutationEvents = new Map<string, number>()

/** Derive boolean flags (`isRead`, `isFlagged`) from raw keyword strings. */
export function deriveKeywordState(keywords: string[]): KeywordState {
  return {
    isFlagged: keywords.includes('$flagged'),
    isRead: keywords.includes('$seen'),
    keywords,
  }
}

/**
 * Normalize a backend conversation page into a cache slice,
 * extracting each summary into its own query entry.
 * @spec docs/L1-ui#data-fetching
 */
export function normalizeConversationPage(
  queryClient: QueryClient,
  page: ConversationPage,
): ConversationPageSlice {
  upsertConversationSummaries(queryClient, page.items)
  return {
    itemIds: page.items.map((item) => item.id),
    nextCursor: page.nextCursor,
  }
}

/** Write each conversation summary into its own React Query entry. */
export function upsertConversationSummaries(
  queryClient: QueryClient,
  conversations: ConversationSummary[],
) {
  for (const conversation of conversations) {
    queryClient.setQueryData(
      mailKeys.conversationSummary(conversation.id),
      conversation,
    )
  }
}

/** Read a cached conversation summary by ID. */
export function getConversationSummary(
  queryClient: QueryClient,
  conversationId: string,
): ConversationSummary | undefined {
  return queryClient.getQueryData<ConversationSummary>(
    mailKeys.conversationSummary(conversationId),
  )
}

function buildLocalMutationKey(
  event: Pick<DomainEvent, 'accountId' | 'messageId' | 'topic'>,
) {
  return `${event.accountId}:${event.messageId ?? 'none'}:${event.topic}`
}

function cleanupLocalMutationEvents(now: number) {
  for (const [key, expiresAt] of localMutationEvents) {
    if (expiresAt <= now) {
      localMutationEvents.delete(key)
    }
  }
}

/**
 * Record events from a locally initiated mutation so they can be
 * suppressed when echoed back via SSE.
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function recordLocalMutationEvents(events: DomainEvent[]) {
  const now = Date.now()
  cleanupLocalMutationEvents(now)
  for (const event of events) {
    if (!event.messageId) {
      continue
    }
    localMutationEvents.set(
      buildLocalMutationKey(event),
      now + LOCAL_MUTATION_TTL_MS,
    )
  }
}

/**
 * Returns true if this SSE event was caused by a recent local mutation
 * and should be ignored to prevent double-application.
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function shouldSuppressLocalEcho(event: DomainEvent): boolean {
  if (!event.messageId) {
    return false
  }

  const now = Date.now()
  cleanupLocalMutationEvents(now)
  const key = buildLocalMutationKey(event)
  const expiresAt = localMutationEvents.get(key)
  if (!expiresAt || expiresAt <= now) {
    localMutationEvents.delete(key)
    return false
  }
  localMutationEvents.delete(key)
  return true
}

function snapshotQuery(
  queryClient: QueryClient,
  queryKey: QueryKey,
): QuerySnapshot {
  const state = queryClient.getQueryState(queryKey)
  return {
    data: queryClient.getQueryData(queryKey),
    existed: state !== undefined,
    queryKey,
  }
}

/** Restore previously snapshotted query entries (used for optimistic rollback). */
export function restoreSnapshots(
  queryClient: QueryClient,
  snapshots: QuerySnapshot[],
) {
  for (const snapshot of snapshots) {
    if (snapshot.existed) {
      queryClient.setQueryData(snapshot.queryKey, snapshot.data)
      continue
    }
    queryClient.removeQueries({ queryKey: snapshot.queryKey, exact: true })
  }
}

function replaceMessageKeywords<T extends MessageSummary | MessageDetail>(
  message: T,
  keywordState: KeywordState,
): T {
  return {
    ...message,
    isFlagged: keywordState.isFlagged,
    isRead: keywordState.isRead,
    keywords: keywordState.keywords,
  }
}

function patchMessageListQueries(
  queryClient: QueryClient,
  target: SourceMessageRef,
  keywordState: KeywordState,
): QuerySnapshot[] {
  const snapshots: QuerySnapshot[] = []
  for (const [queryKey, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({
    queryKey: queryKeys.messagesRoot,
  })) {
    const hasTarget = data?.pages.some((page) =>
      page.items.some(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      ),
    )
    if (!data || !hasTarget) {
      continue
    }

    snapshots.push(snapshotQuery(queryClient, queryKey))
    queryClient.setQueryData<InfiniteData<MessagePage, string | null>>(
      queryKey,
      {
        ...data,
        pages: data.pages.map((page) => ({
          ...page,
          items: page.items.map((message) =>
            message.sourceId === target.sourceId &&
            message.id === target.messageId
              ? replaceMessageKeywords(message, keywordState)
              : message,
          ),
        })),
      },
    )
  }
  return snapshots
}

/**
 * Heuristically patch a conversation summary for a single-message keyword change
 * when the full conversation view is not cached.
 */
function applyHeuristicConversationPatch(
  conversation: ConversationSummary,
  patch: KeywordPatch,
  options?: ReconcileOptions,
): { conversation: ConversationSummary; incomplete: boolean } {
  let incomplete = false
  let nextConversation = conversation

  if (patch.previous.isRead !== patch.next.isRead) {
    const unreadDelta = patch.next.isRead ? -1 : 1
    nextConversation = {
      ...nextConversation,
      unreadCount: Math.max(0, nextConversation.unreadCount + unreadDelta),
    }
  }

  if (patch.previous.isFlagged !== patch.next.isFlagged) {
    if (patch.next.isFlagged) {
      nextConversation = { ...nextConversation, isFlagged: true }
    } else if (
      options?.allowHeuristicFlagClear ||
      nextConversation.messageCount <= 1
    ) {
      nextConversation = { ...nextConversation, isFlagged: false }
    } else {
      incomplete = true
    }
  }

  return { conversation: nextConversation, incomplete }
}

/**
 * Derive a conversation summary from a full conversation view.
 * @spec docs/L1-sync#conversation-pagination
 */
function summarizeConversation(
  conversation: ConversationView,
): ConversationSummary {
  const latestMessage = conversation.messages[conversation.messages.length - 1]
  return {
    id: conversation.id,
    subject: conversation.subject ?? latestMessage?.subject ?? null,
    preview: latestMessage?.preview ?? null,
    fromName: latestMessage?.fromName ?? null,
    fromEmail: latestMessage?.fromEmail ?? null,
    latestReceivedAt: latestMessage?.receivedAt ?? '',
    unreadCount: conversation.messages.reduce(
      (count, message) => count + (message.isRead ? 0 : 1),
      0,
    ),
    messageCount: conversation.messages.length,
    sourceIds: [
      ...new Set(conversation.messages.map((message) => message.sourceId)),
    ],
    sourceNames: [
      ...new Set(conversation.messages.map((message) => message.sourceName)),
    ],
    latestMessage: latestMessage
      ? { messageId: latestMessage.id, sourceId: latestMessage.sourceId }
      : { messageId: '', sourceId: '' },
    latestSourceName: latestMessage?.sourceName ?? '',
    hasAttachment: conversation.messages.some(
      (message) => message.hasAttachment,
    ),
    isFlagged: conversation.messages.some((message) => message.isFlagged),
  }
}

/**
 * Apply a keyword patch to a full conversation view and derive the updated summary.
 */
function applyPatchToConversationView(
  conversation: ConversationView,
  target: MailSelection,
  patch: KeywordPatch,
): {
  changed: boolean
  conversation: ConversationView
  summary: ConversationSummary
} {
  let changed = false
  const messages = conversation.messages.map((message) => {
    if (
      message.sourceId !== target.sourceId ||
      message.id !== target.messageId
    ) {
      return message
    }
    changed = true
    return replaceMessageKeywords(message, patch.next)
  })

  const nextConversation = changed
    ? { ...conversation, messages }
    : conversation
  return {
    changed,
    conversation: nextConversation,
    summary: summarizeConversation(nextConversation),
  }
}

/**
 * Optimistically apply a keyword patch across message, conversation, and summary cache entries.
 * Returns rollback snapshots and whether the patch was incomplete (needs server confirmation).
 * @spec docs/L1-ui#data-fetching
 */
export function applyKeywordPatch(
  queryClient: QueryClient,
  target: MailSelection,
  patch: KeywordPatch,
  options?: ReconcileOptions,
): CachePatchResult {
  const snapshots = [
    snapshotQuery(
      queryClient,
      mailKeys.message(target.sourceId, target.messageId),
    ),
    snapshotQuery(queryClient, mailKeys.conversation(target.conversationId)),
    snapshotQuery(
      queryClient,
      mailKeys.conversationSummary(target.conversationId),
    ),
  ]
  snapshots.push(...patchMessageListQueries(queryClient, target, patch.next))

  let incomplete = false
  let exactSummary: ConversationSummary | null = null

  const messageKey = mailKeys.message(target.sourceId, target.messageId)
  const currentMessage = queryClient.getQueryData<MessageDetail>(messageKey)
  if (currentMessage) {
    queryClient.setQueryData<MessageDetail>(
      messageKey,
      replaceMessageKeywords(currentMessage, patch.next),
    )
  } else {
    incomplete = true
  }

  const conversationKey = mailKeys.conversation(target.conversationId)
  const currentConversation =
    queryClient.getQueryData<ConversationView>(conversationKey)
  if (currentConversation) {
    const updatedConversation = applyPatchToConversationView(
      currentConversation,
      target,
      patch,
    )
    queryClient.setQueryData(conversationKey, updatedConversation.conversation)
    exactSummary = updatedConversation.summary
  }

  const currentSummary = getConversationSummary(
    queryClient,
    target.conversationId,
  )
  if (exactSummary) {
    queryClient.setQueryData(
      mailKeys.conversationSummary(target.conversationId),
      currentSummary ? { ...currentSummary, ...exactSummary } : exactSummary,
    )
  } else if (currentSummary) {
    const heuristicResult = applyHeuristicConversationPatch(
      currentSummary,
      patch,
      options,
    )
    queryClient.setQueryData(
      mailKeys.conversationSummary(target.conversationId),
      heuristicResult.conversation,
    )
    incomplete ||= heuristicResult.incomplete
  } else {
    incomplete = true
  }

  return { incomplete, snapshots }
}

/**
 * Merge a fresh message detail into the cache and update the parent conversation summary.
 * @spec docs/L1-ui#data-fetching
 */
export function mergeMessageDetail(
  queryClient: QueryClient,
  detail: MessageDetail,
  conversationId: string,
) {
  queryClient.setQueryData(mailKeys.message(detail.sourceId, detail.id), detail)
  patchMessageListQueries(
    queryClient,
    { messageId: detail.id, sourceId: detail.sourceId },
    deriveKeywordState(detail.keywords),
  )

  const conversationKey = mailKeys.conversation(conversationId)
  const conversation =
    queryClient.getQueryData<ConversationView>(conversationKey)
  if (!conversation) {
    return false
  }

  const messages = conversation.messages.map((message) =>
    message.sourceId === detail.sourceId && message.id === detail.id
      ? replaceMessageKeywords(message, detail)
      : message,
  )
  const nextConversation = { ...conversation, messages }
  queryClient.setQueryData(conversationKey, nextConversation)

  const summary = summarizeConversation(nextConversation)
  const currentSummary = getConversationSummary(queryClient, conversationId)
  queryClient.setQueryData(
    mailKeys.conversationSummary(conversationId),
    currentSummary ? { ...currentSummary, ...summary } : summary,
  )

  return true
}

/** Two string arrays hold the same members regardless of order. */
function sameMembers(a: string[], b: string[]): boolean {
  if (a.length !== b.length) {
    return false
  }
  const set = new Set(a)
  return b.every((value) => set.has(value))
}

/**
 * Read a message's current mutable state from the cache, checking message
 * detail, then any message-list page, then any cached conversation view.
 * Returns null when the message is not present in the cache (the caller then
 * cannot optimistically patch or invert the operation).
 * @spec docs/L1-ui#undo-system
 */
export function captureMutableState(
  queryClient: QueryClient,
  target: SourceMessageRef,
): MutableState | null {
  const detail = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (detail) {
    return { keywords: detail.keywords, mailboxIds: detail.mailboxIds }
  }

  for (const [, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({ queryKey: queryKeys.messagesRoot })) {
    const match = data?.pages
      .flatMap((page) => page.items)
      .find(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      )
    if (match) {
      return { keywords: match.keywords, mailboxIds: match.mailboxIds }
    }
  }

  for (const [, conversation] of queryClient.getQueriesData<ConversationView>({
    queryKey: mailKeys.conversationRoot,
  })) {
    const match = conversation?.messages.find(
      (message) =>
        message.sourceId === target.sourceId && message.id === target.messageId,
    )
    if (match) {
      return { keywords: match.keywords, mailboxIds: match.mailboxIds }
    }
  }

  return null
}

/**
 * Produce the minimal set of commands that drives a message from `current` to
 * `target` state. This is the engine behind both forward moves and undo: an
 * inverse is just a diff back to the captured before-image.
 * @spec docs/L1-ui#undo-system
 */
export function diffMutableState(
  current: MutableState,
  target: MutableState,
): MessageCommand[] {
  const commands: MessageCommand[] = []
  if (!sameMembers(current.mailboxIds, target.mailboxIds)) {
    commands.push({ kind: 'replaceMailboxes', mailboxIds: target.mailboxIds })
  }
  const add = target.keywords.filter(
    (keyword) => !current.keywords.includes(keyword),
  )
  const remove = current.keywords.filter(
    (keyword) => !target.keywords.includes(keyword),
  )
  if (add.length > 0 || remove.length > 0) {
    commands.push({ kind: 'setKeywords', add, remove })
  }
  return commands
}

/**
 * Decide whether a message with `nextMailboxIds` still belongs in the list view
 * identified by `queryKey`. Source-mailbox views are decidable from membership;
 * smart-mailbox and unscoped views are not (returns null → caller marks the
 * patch incomplete and falls back to invalidation).
 */
function messageBelongsToListView(
  queryKey: QueryKey,
  nextMailboxIds: string[],
): boolean | null {
  const selection = queryKey[1] as
    | { kind: 'source-mailbox'; sourceId: string; mailboxId: string }
    | { kind: 'smart-mailbox'; id: string }
    | null
    | undefined
  if (!selection) {
    return null
  }
  if (selection.kind === 'source-mailbox') {
    return nextMailboxIds.includes(selection.mailboxId)
  }
  return null
}

function patchMessageListMembership(
  queryClient: QueryClient,
  target: SourceMessageRef,
  nextMailboxIds: string[],
  destroy: boolean,
): { incomplete: boolean; snapshots: QuerySnapshot[] } {
  const snapshots: QuerySnapshot[] = []
  let incomplete = false
  for (const [queryKey, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({ queryKey: queryKeys.messagesRoot })) {
    const hasTarget = data?.pages.some((page) =>
      page.items.some(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      ),
    )
    if (!data || !hasTarget) {
      continue
    }

    const belongs = destroy
      ? false
      : messageBelongsToListView(queryKey, nextMailboxIds)
    if (belongs === null) {
      // Membership cannot be decided from the cache (smart mailbox / unscoped):
      // leave the row in place and let server reconciliation settle it.
      incomplete = true
    }

    snapshots.push(snapshotQuery(queryClient, queryKey))
    queryClient.setQueryData<InfiniteData<MessagePage, string | null>>(
      queryKey,
      {
        ...data,
        pages: data.pages.map((page) => ({
          ...page,
          items:
            belongs === false
              ? page.items.filter(
                  (message) =>
                    !(
                      message.sourceId === target.sourceId &&
                      message.id === target.messageId
                    ),
                )
              : page.items.map((message) =>
                  message.sourceId === target.sourceId &&
                  message.id === target.messageId
                    ? { ...message, mailboxIds: nextMailboxIds }
                    : message,
                ),
        })),
      },
    )
  }
  return { incomplete, snapshots }
}

/**
 * Optimistically apply a mailbox-membership change (move/archive/trash) or a
 * destroy across message detail, conversation view + summary, and list views.
 * Mirrors {@link applyKeywordPatch}: returns rollback snapshots and whether the
 * patch was incomplete and needs server confirmation.
 * @spec docs/L1-ui#data-fetching
 */
export function applyMailboxPatch(
  queryClient: QueryClient,
  target: MailSelection,
  nextMailboxIds: string[],
  options?: { destroy?: boolean },
): CachePatchResult {
  const destroy = options?.destroy ?? false
  const snapshots: QuerySnapshot[] = []
  let incomplete = false

  const messageKey = mailKeys.message(target.sourceId, target.messageId)
  const currentMessage = queryClient.getQueryData<MessageDetail>(messageKey)
  if (currentMessage) {
    snapshots.push(snapshotQuery(queryClient, messageKey))
    if (destroy) {
      queryClient.removeQueries({ queryKey: messageKey, exact: true })
    } else {
      queryClient.setQueryData<MessageDetail>(messageKey, {
        ...currentMessage,
        mailboxIds: nextMailboxIds,
      })
    }
  }

  const conversationKey = mailKeys.conversation(target.conversationId)
  const conversation =
    queryClient.getQueryData<ConversationView>(conversationKey)
  if (conversation) {
    const messages = destroy
      ? conversation.messages.filter(
          (message) =>
            !(
              message.sourceId === target.sourceId &&
              message.id === target.messageId
            ),
        )
      : conversation.messages.map((message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId
            ? { ...message, mailboxIds: nextMailboxIds }
            : message,
        )
    snapshots.push(snapshotQuery(queryClient, conversationKey))
    queryClient.setQueryData(conversationKey, { ...conversation, messages })

    const summaryKey = mailKeys.conversationSummary(target.conversationId)
    snapshots.push(snapshotQuery(queryClient, summaryKey))
    if (messages.length === 0) {
      queryClient.removeQueries({ queryKey: summaryKey, exact: true })
    } else {
      queryClient.setQueryData(
        summaryKey,
        summarizeConversation({ ...conversation, messages }),
      )
    }
  }

  const listResult = patchMessageListMembership(
    queryClient,
    target,
    nextMailboxIds,
    destroy,
  )
  snapshots.push(...listResult.snapshots)
  incomplete ||= listResult.incomplete

  return { incomplete, snapshots }
}

/**
 * Look up a conversation ID for a message by checking cached detail,
 * conversation views, and conversation summaries.
 */
export function findConversationIdForMessage(
  queryClient: QueryClient,
  target: SourceMessageRef,
): string | null {
  const cachedMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (cachedMessage) {
    return cachedMessage.conversationId
  }

  for (const [, data] of queryClient.getQueriesData<
    InfiniteData<MessagePage, string | null>
  >({ queryKey: queryKeys.messagesRoot })) {
    const match = data?.pages
      .flatMap((page) => page.items)
      .find(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      )
    if (match) {
      return match.conversationId
    }
  }

  for (const [, conversation] of queryClient.getQueriesData<ConversationView>({
    queryKey: mailKeys.conversationRoot,
  })) {
    if (
      conversation?.messages.some(
        (message) =>
          message.sourceId === target.sourceId &&
          message.id === target.messageId,
      )
    ) {
      return conversation.id
    }
  }

  for (const [, summary] of queryClient.getQueriesData<ConversationSummary>({
    queryKey: mailKeys.conversationSummaryRoot,
  })) {
    if (
      summary?.latestMessage.sourceId === target.sourceId &&
      summary.latestMessage.messageId === target.messageId
    ) {
      return summary.id
    }
  }

  return null
}

/**
 * Apply a keyword change from an SSE event by resolving the conversation
 * from the cache and delegating to {@link applyKeywordPatch}.
 * @spec docs/L1-ui#live-prepend-behavior
 */
export function applyKeywordEventPatch(
  queryClient: QueryClient,
  target: SourceMessageRef,
  keywords: string[],
): boolean {
  const conversationId = findConversationIdForMessage(queryClient, target)
  if (!conversationId) {
    return false
  }

  const currentMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  )
  if (!currentMessage) {
    return false
  }

  applyKeywordPatch(
    queryClient,
    { ...target, conversationId },
    {
      next: deriveKeywordState(keywords),
      previous: deriveKeywordState(currentMessage.keywords),
    },
  )
  return true
}

/**
 * Write a full conversation view into the cache and update the derived summary.
 * @spec docs/L1-ui#data-fetching
 */
export function mergeConversationView(
  queryClient: QueryClient,
  conversation: ConversationView,
) {
  queryClient.setQueryData(mailKeys.conversation(conversation.id), conversation)
  queryClient.setQueryData(
    mailKeys.conversationSummary(conversation.id),
    summarizeConversation(conversation),
  )
}

/**
 * Flatten all pages of an infinite conversation query into a single ID array.
 * @spec docs/L1-ui#messagelist
 */
export function readConversationIds(
  data: InfiniteData<ConversationPageSlice, unknown> | undefined,
): string[] {
  return data?.pages.flatMap((page) => page.itemIds) ?? []
}
