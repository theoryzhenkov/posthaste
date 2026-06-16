import type {
  AccountOverview,
  ConversationView,
  Mailbox,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  ReadResponse,
  SmartMailboxSummary,
} from '../api/types'

import type { RuntimeEventHandlers, RuntimeTriggerSyncResult } from './types'
import {
  defaultAccounts,
  defaultMailboxes,
  defaultMessageCommandResult,
  defaultMessagePage,
  defaultReadResponse,
  defaultSmartMailboxes,
  queueReject,
  queueResolve,
  resolveQueued,
  resolveQueuedOptional,
  type FakeRuntimeAdapter,
  type FakeRuntimeAdapterOptions,
  type QueuedOutcome,
} from './fakeAdapterSupport'

export type { FakeRuntimeAdapter } from './fakeAdapterSupport'

/** Fake runtime adapter for renderer tests. Does not start or contact a backend. */
export function createFakeRuntimeAdapter(
  input?: FakeRuntimeAdapterOptions,
): FakeRuntimeAdapter {
  const calls = createCallRecords()
  const queues = createQueues()
  const eventHandlers = new Set<RuntimeEventHandlers>()
  let accountCalls = 0
  let smartMailboxCalls = 0

  return {
    get accountCalls() {
      return accountCalls
    },
    ...calls,
    get smartMailboxCalls() {
      return smartMailboxCalls
    },
    emitDomainEvent(event) {
      for (const handlers of eventHandlers) handlers.onEvent(event)
    },
    queueAccounts(accounts) {
      queueResolve(queues.accounts, accounts)
    },
    queueAccountsError(error) {
      queueReject(queues.accounts, error)
    },
    queueConversation(conversation) {
      queueResolve(queues.conversations, conversation)
    },
    queueConversationError(error) {
      queueReject(queues.conversations, error)
    },
    queueMailboxes(mailboxes) {
      queueResolve(queues.mailboxes, mailboxes)
    },
    queueMailboxesError(error) {
      queueReject(queues.mailboxes, error)
    },
    queueMessage(message) {
      queueResolve(queues.messages, message)
    },
    queueMessageError(error) {
      queueReject(queues.messages, error)
    },
    queueMessageCommandResult(result) {
      queueResolve(queues.messageCommands, result)
    },
    queueMessageCommandError(error) {
      queueReject(queues.messageCommands, error)
    },
    queueMessagePage(page) {
      queueResolve(queues.messagePages, page)
    },
    queueMessagePageError(error) {
      queueReject(queues.messagePages, error)
    },
    queueResourceBlob(blob) {
      queueResolve(queues.resources, blob)
    },
    queueResourceError(error) {
      queueReject(queues.resources, error)
    },
    queueReadResponse(response) {
      queueResolve(queues.reads, response)
    },
    queueReadError(error) {
      queueReject(queues.reads, error)
    },
    queueSmartMailboxes(mailboxes) {
      queueResolve(queues.smartMailboxes, mailboxes)
    },
    queueSmartMailboxesError(error) {
      queueReject(queues.smartMailboxes, error)
    },
    queueSyncResult(result) {
      queueResolve(queues.syncs, result)
    },
    queueSyncError(error) {
      queueReject(queues.syncs, error)
    },
    reset() {
      accountCalls = 0
      smartMailboxCalls = 0
      eventHandlers.clear()
      resetCallRecords(calls)
      resetQueues(queues)
    },
    subscribeEvents(request, handlers) {
      calls.eventSubscriptionCalls.push({ request })
      eventHandlers.add(handlers)
      return () => eventHandlers.delete(handlers)
    },
    fetchAccounts() {
      accountCalls += 1
      return resolveQueued(
        queues.accounts,
        input?.defaultAccounts ?? defaultAccounts,
      )
    },
    fetchConversation(conversationId) {
      calls.conversationCalls.push(conversationId)
      return resolveQueuedOptional(
        queues.conversations,
        input?.defaultConversation,
        'conversation result',
      )
    },
    fetchConversationPage() {
      return Promise.resolve({ items: [], nextCursor: null })
    },
    fetchIdentity() {
      return Promise.reject(new Error('fake runtime adapter has no identity'))
    },
    fetchMailboxes(accountId) {
      calls.mailboxCalls.push(accountId)
      return resolveQueued(
        queues.mailboxes,
        input?.defaultMailboxes ?? defaultMailboxes,
      )
    },
    fetchMessage(messageId, sourceId) {
      calls.messageCalls.push({ messageId, sourceId })
      return resolveQueuedOptional(
        queues.messages,
        input?.defaultMessage,
        'message result',
      )
    },
    fetchMessagePage(request) {
      calls.messagePageCalls.push({ ...request })
      return resolveQueued(
        queues.messagePages,
        input?.defaultMessagePage ?? defaultMessagePage,
      )
    },
    fetchReplyContext() {
      return Promise.reject(
        new Error('fake runtime adapter has no reply context'),
      )
    },
    fetchResourceBlob(descriptor) {
      calls.resourceCalls.push({ descriptor })
      return resolveQueuedOptional(
        queues.resources,
        undefined,
        'resource blob result',
      )
    },
    fetchSenderAddresses() {
      return Promise.resolve([])
    },
    fetchSmartMailboxes() {
      smartMailboxCalls += 1
      return resolveQueued(
        queues.smartMailboxes,
        input?.defaultSmartMailboxes ?? defaultSmartMailboxes,
      )
    },
    read(request) {
      calls.readCalls.push(request)
      return resolveQueued(
        queues.reads,
        input?.defaultReadResponse ?? defaultReadResponse,
      )
    },
    runMessageCommand(request) {
      calls.messageCommandCalls.push({ ...request })
      return resolveQueued(
        queues.messageCommands,
        input?.defaultMessageCommandResult ?? defaultMessageCommandResult,
      )
    },
    sendMessage() {
      return Promise.reject(
        new Error('fake runtime adapter has no send message result'),
      )
    },
    triggerSync(request) {
      calls.syncCalls.push(request)
      return resolveQueuedOptional(queues.syncs, undefined, 'sync result')
    },
  }
}

function createCallRecords() {
  return {
    conversationCalls: [] as string[],
    eventSubscriptionCalls: [] as FakeRuntimeAdapter['eventSubscriptionCalls'],
    mailboxCalls: [] as string[],
    messageCalls: [] as FakeRuntimeAdapter['messageCalls'],
    messageCommandCalls: [] as FakeRuntimeAdapter['messageCommandCalls'],
    messagePageCalls: [] as FakeRuntimeAdapter['messagePageCalls'],
    readCalls: [] as FakeRuntimeAdapter['readCalls'],
    resourceCalls: [] as FakeRuntimeAdapter['resourceCalls'],
    syncCalls: [] as FakeRuntimeAdapter['syncCalls'],
  }
}

function resetCallRecords(records: ReturnType<typeof createCallRecords>): void {
  for (const value of Object.values(records)) value.length = 0
}

function createQueues() {
  return {
    accounts: [] as QueuedOutcome<AccountOverview[]>[],
    conversations: [] as QueuedOutcome<ConversationView>[],
    mailboxes: [] as QueuedOutcome<Mailbox[]>[],
    messages: [] as QueuedOutcome<MessageDetail>[],
    messageCommands: [] as QueuedOutcome<MessageCommandResult>[],
    messagePages: [] as QueuedOutcome<MessagePage>[],
    reads: [] as QueuedOutcome<ReadResponse>[],
    resources: [] as QueuedOutcome<Blob>[],
    smartMailboxes: [] as QueuedOutcome<SmartMailboxSummary[]>[],
    syncs: [] as QueuedOutcome<RuntimeTriggerSyncResult>[],
  }
}

function resetQueues(queues: ReturnType<typeof createQueues>): void {
  for (const value of Object.values(queues)) value.length = 0
}
