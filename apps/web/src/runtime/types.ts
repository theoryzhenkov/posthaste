import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CachedSenderAddress,
  ConversationPage,
  ConversationView,
  DomainEvent,
  CreateSmartMailboxInput,
  Mailbox,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSortField,
  OkResponse,
  PatchMailboxInput,
  ReadRequest,
  ReplyContext,
  ReadResponse,
  SendMessageInput,
  SmartMailbox,
  SmartMailboxSummary,
  SyncMode,
  Identity,
  UpdateSmartMailboxInput,
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

export interface RuntimeTriggerSyncRequest {
  sourceId: string
  mode?: SyncMode
}

export interface RuntimeTriggerSyncResult {
  ok: boolean
  eventCount: number
  mode: SyncMode
}

export interface RuntimeConversationPageRequest {
  sourceId?: string | null
  mailboxId?: string | null
  limit?: number
  cursor?: string | null
  sort?: string
  sortDir?: string
  q?: string
}

export interface RuntimeReplyContextRequest {
  sourceId: string
  messageId: string
}

export interface RuntimeSendMessageRequest {
  sourceId: string
  input: SendMessageInput
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
  createSmartMailbox(input: CreateSmartMailboxInput): Promise<SmartMailbox>
  deleteSmartMailbox(smartMailboxId: string): Promise<OkResponse>
  fetchAccounts(): Promise<AccountOverview[]>
  fetchConversation(conversationId: string): Promise<ConversationView>
  fetchConversationPage(
    request?: RuntimeConversationPageRequest,
  ): Promise<ConversationPage>
  fetchIdentity(sourceId: string): Promise<Identity>
  fetchMailboxes(accountId: string): Promise<Mailbox[]>
  fetchMessage(messageId: string, sourceId: string): Promise<MessageDetail>
  fetchMessagePage(request: RuntimeMessagePageRequest): Promise<MessagePage>
  fetchSettings(): Promise<AppSettings>
  fetchReplyContext(request: RuntimeReplyContextRequest): Promise<ReplyContext>
  fetchResourceBlob(
    descriptor: RuntimeResourceDescriptor,
    options?: RuntimeResourceFetchOptions,
  ): Promise<Blob>
  fetchSenderAddresses(): Promise<CachedSenderAddress[]>
  fetchSmartMailbox(smartMailboxId: string): Promise<SmartMailbox>
  fetchSmartMailboxes(): Promise<SmartMailboxSummary[]>
  patchMailbox(
    accountId: string,
    mailboxId: string,
    input: PatchMailboxInput,
  ): Promise<Mailbox[]>
  patchSettings(input: Partial<AppSettings>): Promise<AppSettings>
  previewAutomationRule(
    input: AutomationRulePreviewInput,
  ): Promise<AutomationRulePreviewResponse>
  read(request: ReadRequest): Promise<ReadResponse>
  resetDefaultSmartMailboxes(): Promise<SmartMailboxSummary[]>
  runMessageCommand(
    request: RuntimeMessageCommandRequest,
  ): Promise<MessageCommandResult>
  sendMessage(request: RuntimeSendMessageRequest): Promise<OkResponse>
  updateSmartMailbox(
    smartMailboxId: string,
    input: UpdateSmartMailboxInput,
  ): Promise<SmartMailbox>
  triggerSync(
    request: RuntimeTriggerSyncRequest,
  ): Promise<RuntimeTriggerSyncResult>
}
