import type {
  AccountOverview,
  ConversationView,
  DomainEvent,
  Mailbox,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  ReadRequest,
  ReadResponse,
  SmartMailboxSummary,
} from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeEventHandlers,
  RuntimeEventSubscriptionRequest,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
  RuntimeResourceDescriptor,
  RuntimeTriggerSyncRequest,
  RuntimeTriggerSyncResult,
} from './types'

type EventSubscriptionCall = { request: RuntimeEventSubscriptionRequest }
type MessageDetailCall = { messageId: string; sourceId: string }
type ResourceCall = { descriptor: RuntimeResourceDescriptor }

type QueuedOutcome<T> =
  | { kind: 'resolve'; value: T }
  | { kind: 'reject'; error: Error }

const defaultAccounts: AccountOverview[] = []
const defaultReadResponse: ReadResponse = { results: {} }
const defaultMailboxes: Mailbox[] = []
const defaultSmartMailboxes: SmartMailboxSummary[] = []

const defaultMessagePage: MessagePage = {
  items: [],
  nextCursor: null,
}

const defaultMessageCommandResult: MessageCommandResult = {
  detail: null,
  events: [],
}

function queueResolve<T>(queue: QueuedOutcome<T>[], value: T): void {
  queue.push({ kind: 'resolve', value })
}

function queueReject<T>(queue: QueuedOutcome<T>[], error: Error): void {
  queue.push({ kind: 'reject', error })
}

function resolveQueued<T>(queue: QueuedOutcome<T>[], fallback: T): Promise<T> {
  const next = queue.shift()
  if (!next) return Promise.resolve(fallback)
  if (next.kind === 'reject') return Promise.reject(next.error)
  return Promise.resolve(next.value)
}

function resolveQueuedOptional<T>(
  queue: QueuedOutcome<T>[],
  fallback: T | undefined,
  label: string,
): Promise<T> {
  const next = queue.shift()
  if (next?.kind === 'reject') return Promise.reject(next.error)
  if (next?.kind === 'resolve') return Promise.resolve(next.value)
  if (fallback !== undefined) return Promise.resolve(fallback)
  return Promise.reject(new Error(`fake runtime adapter has no ${label}`))
}

export interface FakeRuntimeAdapter extends RuntimeAdapter {
  readonly accountCalls: number
  readonly conversationCalls: string[]
  readonly eventSubscriptionCalls: EventSubscriptionCall[]
  readonly mailboxCalls: string[]
  readonly messageCalls: MessageDetailCall[]
  readonly messageCommandCalls: RuntimeMessageCommandRequest[]
  readonly messagePageCalls: RuntimeMessagePageRequest[]
  readonly readCalls: ReadRequest[]
  readonly resourceCalls: ResourceCall[]
  readonly smartMailboxCalls: number
  readonly syncCalls: RuntimeTriggerSyncRequest[]
  emitDomainEvent(event: DomainEvent): void
  queueAccounts(accounts: AccountOverview[]): void
  queueAccountsError(error: Error): void
  queueConversation(conversation: ConversationView): void
  queueConversationError(error: Error): void
  queueMailboxes(mailboxes: Mailbox[]): void
  queueMailboxesError(error: Error): void
  queueMessage(message: MessageDetail): void
  queueMessageError(error: Error): void
  queueMessageCommandResult(result: MessageCommandResult): void
  queueMessageCommandError(error: Error): void
  queueMessagePage(page: MessagePage): void
  queueMessagePageError(error: Error): void
  queueResourceBlob(blob: Blob): void
  queueResourceError(error: Error): void
  queueReadResponse(response: ReadResponse): void
  queueReadError(error: Error): void
  queueSmartMailboxes(mailboxes: SmartMailboxSummary[]): void
  queueSmartMailboxesError(error: Error): void
  queueSyncResult(result: RuntimeTriggerSyncResult): void
  queueSyncError(error: Error): void
  reset(): void
}

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(input?: {
  defaultAccounts?: AccountOverview[]
  defaultConversation?: ConversationView
  defaultMailboxes?: Mailbox[]
  defaultMessage?: MessageDetail
  defaultMessageCommandResult?: MessageCommandResult
  defaultMessagePage?: MessagePage
  defaultReadResponse?: ReadResponse
  defaultSmartMailboxes?: SmartMailboxSummary[]
}): FakeRuntimeAdapter {
  const conversationCalls: string[] = []
  const eventSubscriptionCalls: EventSubscriptionCall[] = []
  const eventHandlers = new Set<RuntimeEventHandlers>()
  const mailboxCalls: string[] = []
  const messageCalls: MessageDetailCall[] = []
  const messageCommandCalls: RuntimeMessageCommandRequest[] = []
  const messagePageCalls: RuntimeMessagePageRequest[] = []
  const readCalls: ReadRequest[] = []
  const resourceCalls: ResourceCall[] = []
  const syncCalls: RuntimeTriggerSyncRequest[] = []
  const queuedAccounts: QueuedOutcome<AccountOverview[]>[] = []
  const queuedConversations: QueuedOutcome<ConversationView>[] = []
  const queuedMailboxes: QueuedOutcome<Mailbox[]>[] = []
  const queuedMessages: QueuedOutcome<MessageDetail>[] = []
  const queuedMessageCommands: QueuedOutcome<MessageCommandResult>[] = []
  const queuedMessagePages: QueuedOutcome<MessagePage>[] = []
  const queuedReads: QueuedOutcome<ReadResponse>[] = []
  const queuedResources: QueuedOutcome<Blob>[] = []
  const queuedSmartMailboxes: QueuedOutcome<SmartMailboxSummary[]>[] = []
  const queuedSyncs: QueuedOutcome<RuntimeTriggerSyncResult>[] = []
  let accountCalls = 0
  let smartMailboxCalls = 0

  return {
    get accountCalls() {
      return accountCalls
    },
    conversationCalls,
    eventSubscriptionCalls,
    mailboxCalls,
    messageCalls,
    messageCommandCalls,
    messagePageCalls,
    readCalls,
    resourceCalls,
    syncCalls,
    get smartMailboxCalls() {
      return smartMailboxCalls
    },
    emitDomainEvent(event) {
      for (const handlers of eventHandlers) handlers.onEvent(event)
    },
    queueAccounts(accounts) {
      queueResolve(queuedAccounts, accounts)
    },
    queueAccountsError(error) {
      queueReject(queuedAccounts, error)
    },
    queueConversation(conversation) {
      queueResolve(queuedConversations, conversation)
    },
    queueConversationError(error) {
      queueReject(queuedConversations, error)
    },
    queueMailboxes(mailboxes) {
      queueResolve(queuedMailboxes, mailboxes)
    },
    queueMailboxesError(error) {
      queueReject(queuedMailboxes, error)
    },
    queueMessage(message) {
      queueResolve(queuedMessages, message)
    },
    queueMessageError(error) {
      queueReject(queuedMessages, error)
    },
    queueMessageCommandResult(result) {
      queueResolve(queuedMessageCommands, result)
    },
    queueMessageCommandError(error) {
      queueReject(queuedMessageCommands, error)
    },
    queueMessagePage(page) {
      queueResolve(queuedMessagePages, page)
    },
    queueMessagePageError(error) {
      queueReject(queuedMessagePages, error)
    },
    queueResourceBlob(blob) {
      queueResolve(queuedResources, blob)
    },
    queueResourceError(error) {
      queueReject(queuedResources, error)
    },
    queueReadResponse(response) {
      queueResolve(queuedReads, response)
    },
    queueReadError(error) {
      queueReject(queuedReads, error)
    },
    queueSmartMailboxes(mailboxes) {
      queueResolve(queuedSmartMailboxes, mailboxes)
    },
    queueSmartMailboxesError(error) {
      queueReject(queuedSmartMailboxes, error)
    },
    queueSyncResult(result) {
      queueResolve(queuedSyncs, result)
    },
    queueSyncError(error) {
      queueReject(queuedSyncs, error)
    },
    reset() {
      accountCalls = 0
      conversationCalls.length = 0
      eventHandlers.clear()
      eventSubscriptionCalls.length = 0
      mailboxCalls.length = 0
      messageCalls.length = 0
      messageCommandCalls.length = 0
      messagePageCalls.length = 0
      readCalls.length = 0
      resourceCalls.length = 0
      smartMailboxCalls = 0
      syncCalls.length = 0
      queuedAccounts.length = 0
      queuedConversations.length = 0
      queuedMailboxes.length = 0
      queuedMessages.length = 0
      queuedMessageCommands.length = 0
      queuedMessagePages.length = 0
      queuedReads.length = 0
      queuedResources.length = 0
      queuedSmartMailboxes.length = 0
      queuedSyncs.length = 0
    },
    subscribeEvents(request, handlers) {
      eventSubscriptionCalls.push({ request })
      eventHandlers.add(handlers)
      return () => eventHandlers.delete(handlers)
    },
    fetchAccounts() {
      accountCalls += 1
      return resolveQueued(
        queuedAccounts,
        input?.defaultAccounts ?? defaultAccounts,
      )
    },
    fetchConversation(conversationId) {
      conversationCalls.push(conversationId)
      return resolveQueuedOptional(
        queuedConversations,
        input?.defaultConversation,
        'conversation result',
      )
    },
    fetchMailboxes(accountId) {
      mailboxCalls.push(accountId)
      return resolveQueued(
        queuedMailboxes,
        input?.defaultMailboxes ?? defaultMailboxes,
      )
    },
    fetchMessage(messageId, sourceId) {
      messageCalls.push({ messageId, sourceId })
      return resolveQueuedOptional(
        queuedMessages,
        input?.defaultMessage,
        'message result',
      )
    },
    fetchMessagePage(request) {
      messagePageCalls.push({ ...request })
      return resolveQueued(
        queuedMessagePages,
        input?.defaultMessagePage ?? defaultMessagePage,
      )
    },
    fetchResourceBlob(descriptor) {
      resourceCalls.push({ descriptor })
      return resolveQueuedOptional(
        queuedResources,
        undefined,
        'resource blob result',
      )
    },
    fetchSmartMailboxes() {
      smartMailboxCalls += 1
      return resolveQueued(
        queuedSmartMailboxes,
        input?.defaultSmartMailboxes ?? defaultSmartMailboxes,
      )
    },
    read(request) {
      readCalls.push(request)
      return resolveQueued(
        queuedReads,
        input?.defaultReadResponse ?? defaultReadResponse,
      )
    },
    runMessageCommand(request) {
      messageCommandCalls.push({ ...request })
      return resolveQueued(
        queuedMessageCommands,
        input?.defaultMessageCommandResult ?? defaultMessageCommandResult,
      )
    },
    triggerSync(request) {
      syncCalls.push(request)
      return resolveQueuedOptional(queuedSyncs, undefined, 'sync result')
    },
  }
}
