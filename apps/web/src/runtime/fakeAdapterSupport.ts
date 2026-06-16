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
  RuntimeEventSubscriptionRequest,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
  RuntimeResourceDescriptor,
  RuntimeTriggerSyncRequest,
  RuntimeTriggerSyncResult,
} from './types'

export type EventSubscriptionCall = {
  request: RuntimeEventSubscriptionRequest
}
export type MessageDetailCall = { messageId: string; sourceId: string }
export type ResourceCall = { descriptor: RuntimeResourceDescriptor }

export type QueuedOutcome<T> =
  | { kind: 'resolve'; value: T }
  | { kind: 'reject'; error: Error }

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

export interface FakeRuntimeAdapterOptions {
  defaultAccounts?: AccountOverview[]
  defaultConversation?: ConversationView
  defaultMailboxes?: Mailbox[]
  defaultMessage?: MessageDetail
  defaultMessageCommandResult?: MessageCommandResult
  defaultMessagePage?: MessagePage
  defaultReadResponse?: ReadResponse
  defaultSmartMailboxes?: SmartMailboxSummary[]
}

export const defaultAccounts: AccountOverview[] = []
export const defaultReadResponse: ReadResponse = { results: {} }
export const defaultMailboxes: Mailbox[] = []
export const defaultSmartMailboxes: SmartMailboxSummary[] = []

export const defaultMessagePage: MessagePage = {
  items: [],
  nextCursor: null,
}

export const defaultMessageCommandResult: MessageCommandResult = {
  detail: null,
  events: [],
}

export function queueResolve<T>(queue: QueuedOutcome<T>[], value: T): void {
  queue.push({ kind: 'resolve', value })
}

export function queueReject<T>(queue: QueuedOutcome<T>[], error: Error): void {
  queue.push({ kind: 'reject', error })
}

export function resolveQueued<T>(
  queue: QueuedOutcome<T>[],
  fallback: T,
): Promise<T> {
  const next = queue.shift()
  if (!next) return Promise.resolve(fallback)
  if (next.kind === 'reject') return Promise.reject(next.error)
  return Promise.resolve(next.value)
}

export function resolveQueuedOptional<T>(
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
