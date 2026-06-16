import type {
  ConversationView,
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
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
} from './types'

type MessageDetailCall = { messageId: string; sourceId: string }

type QueuedOutcome<T> =
  | { kind: 'resolve'; value: T }
  | { kind: 'reject'; error: Error }

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
  if (!next) {
    return Promise.resolve(fallback)
  }
  if (next.kind === 'reject') {
    return Promise.reject(next.error)
  }
  return Promise.resolve(next.value)
}

function resolveQueuedOptional<T>(
  queue: QueuedOutcome<T>[],
  fallback: T | undefined,
  label: string,
): Promise<T> {
  const next = queue.shift()
  if (next?.kind === 'reject') {
    return Promise.reject(next.error)
  }
  if (next?.kind === 'resolve') {
    return Promise.resolve(next.value)
  }
  if (fallback !== undefined) {
    return Promise.resolve(fallback)
  }
  return Promise.reject(new Error(`fake runtime adapter has no ${label}`))
}

export interface FakeRuntimeAdapter extends RuntimeAdapter {
  readonly conversationCalls: string[]
  readonly mailboxCalls: string[]
  readonly messageCalls: MessageDetailCall[]
  readonly messageCommandCalls: RuntimeMessageCommandRequest[]
  readonly messagePageCalls: RuntimeMessagePageRequest[]
  readonly readCalls: ReadRequest[]
  readonly smartMailboxCalls: number
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
  queueReadResponse(response: ReadResponse): void
  queueReadError(error: Error): void
  queueSmartMailboxes(mailboxes: SmartMailboxSummary[]): void
  queueSmartMailboxesError(error: Error): void
  reset(): void
}

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(input?: {
  defaultConversation?: ConversationView
  defaultMailboxes?: Mailbox[]
  defaultMessage?: MessageDetail
  defaultMessageCommandResult?: MessageCommandResult
  defaultMessagePage?: MessagePage
  defaultReadResponse?: ReadResponse
  defaultSmartMailboxes?: SmartMailboxSummary[]
}): FakeRuntimeAdapter {
  const conversationCalls: string[] = []
  const mailboxCalls: string[] = []
  const messageCalls: MessageDetailCall[] = []
  const messageCommandCalls: RuntimeMessageCommandRequest[] = []
  const messagePageCalls: RuntimeMessagePageRequest[] = []
  const readCalls: ReadRequest[] = []
  const queuedConversations: QueuedOutcome<ConversationView>[] = []
  const queuedMailboxes: QueuedOutcome<Mailbox[]>[] = []
  const queuedMessages: QueuedOutcome<MessageDetail>[] = []
  const queuedMessageCommands: QueuedOutcome<MessageCommandResult>[] = []
  const queuedMessagePages: QueuedOutcome<MessagePage>[] = []
  const queuedReads: QueuedOutcome<ReadResponse>[] = []
  const queuedSmartMailboxes: QueuedOutcome<SmartMailboxSummary[]>[] = []
  let smartMailboxCalls = 0

  return {
    conversationCalls,
    mailboxCalls,
    messageCalls,
    messageCommandCalls,
    messagePageCalls,
    readCalls,
    get smartMailboxCalls() {
      return smartMailboxCalls
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
    reset() {
      conversationCalls.length = 0
      mailboxCalls.length = 0
      messageCalls.length = 0
      messageCommandCalls.length = 0
      messagePageCalls.length = 0
      readCalls.length = 0
      queuedConversations.length = 0
      queuedMailboxes.length = 0
      queuedMessages.length = 0
      queuedMessageCommands.length = 0
      queuedMessagePages.length = 0
      queuedReads.length = 0
      queuedSmartMailboxes.length = 0
      smartMailboxCalls = 0
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
  }
}
