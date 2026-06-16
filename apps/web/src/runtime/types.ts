import type {
  ConversationView,
  DomainEvent,
  Mailbox,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSortField,
  ReadRequest,
  ReadResponse,
  SmartMailboxSummary,
} from '../api/types'
import type { OperationContext } from '../observability'

/**
 * Runtime-level request for a message command.
 *
 * This shape is transport-neutral for renderer code: adapters decide whether it
 * is fulfilled by embedded runtime commands or the temporary HTTP bridge.
 */
export interface RuntimeMessageCommandRequest {
  sourceId: string
  messageId: string
  command: MessageCommand
}

export type RuntimeMessagePageScope =
  | { kind: 'source-mailbox'; sourceId: string; mailboxId: string | null }
  | { kind: 'smart-mailbox'; smartMailboxId: string }
  | { kind: 'global' }

export interface RuntimeMessagePageRequest {
  scope: RuntimeMessagePageScope
  query?: string
  cursor?: string | null
  limit: number
  sort?: MessageSortField | 'relevance'
  sortDir?: 'asc' | 'desc'
  signal?: AbortSignal
  operation: OperationContext
}

export type RuntimeResourceDescriptor =
  | { kind: 'account-logo'; imageId: string }
  | {
      kind: 'message-attachment'
      sourceId: string
      messageId: string
      attachmentId: string
    }

export interface RuntimeResourceFetchOptions {
  signal?: AbortSignal
}

export interface RuntimeEventSubscriptionRequest {
  afterSeq?: number | null
}

export interface RuntimeEventHandlers {
  onEvent(event: DomainEvent): void
  onMalformedFrame?(input: { raw: string; error: unknown }): void
  onPermanentError?(error: unknown): void
  onTransientError?(error: unknown): void
  onClosed?(error: unknown): void
}

export type RuntimeUnsubscribe = () => void

/** Renderer-facing runtime adapter facade. */
export interface RuntimeAdapter {
  subscribeEvents(
    request: RuntimeEventSubscriptionRequest,
    handlers: RuntimeEventHandlers,
  ): RuntimeUnsubscribe
  fetchConversation(conversationId: string): Promise<ConversationView>
  fetchMailboxes(accountId: string): Promise<Mailbox[]>
  fetchMessage(messageId: string, sourceId: string): Promise<MessageDetail>
  fetchMessagePage(request: RuntimeMessagePageRequest): Promise<MessagePage>
  fetchResourceBlob(
    descriptor: RuntimeResourceDescriptor,
    options?: RuntimeResourceFetchOptions,
  ): Promise<Blob>
  fetchSmartMailboxes(): Promise<SmartMailboxSummary[]>
  read(request: ReadRequest): Promise<ReadResponse>
  runMessageCommand(
    request: RuntimeMessageCommandRequest,
  ): Promise<MessageCommandResult>
}
