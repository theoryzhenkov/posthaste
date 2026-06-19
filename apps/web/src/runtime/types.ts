import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CachedSenderAddress,
  ConversationPage,
  ConversationView,
  CreateAccountInput,
  DomainEvent,
  CreateSmartMailboxInput,
  Mailbox,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSortField,
  MessageSummary,
  OkResponse,
  PatchMailboxInput,
  ReadRequest,
  ReplyContext,
  ReadResponse,
  SendMessageInput,
  SmartMailbox,
  SmartMailboxSummary,
  StartOAuthResponse,
  StartProviderOAuthInput,
  SyncMode,
  Identity,
  KnownMailboxRole,
  UpdateAccountInput,
  UpdateSmartMailboxInput,
  VerificationResponse,
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

export interface RuntimeMessageCursor {
  sortValue: string
  sourceId: string
  messageId: string
}

export interface RuntimeMailQueryRequest {
  query: string
  presentation: {
    kind: 'messages'
    limit: number | null
    cursor: RuntimeMessageCursor | null
    sortField: MessageSortField
    sortDirection: 'asc' | 'desc'
  }
  visibility: null
}

export interface RuntimeViewSnapshot<TData = unknown> {
  viewId: string
  descriptor: { family: string; payload: unknown }
  revision: number
  lifecycle: 'loading' | 'ready' | 'updating' | 'error'
  readWatermark: { value: string } | null
  coverage: { kind: 'complete' | 'partial' | 'unknown'; details?: unknown }
  data: TData
  pendingMutations: string[]
  error: unknown | null
}

export interface RuntimeMailListRowState {
  rowKey: string
  resourceRef: string | null
  projection: MessageSummary
  sortKey?: unknown
  orderKey: string
  pendingMarkers?: string[]
}

export interface RuntimeMailListViewState {
  scope: unknown
  projectionKind: 'message' | 'conversation'
  sort: unknown
  windowRequest: unknown
  rows: RuntimeMailListRowState[]
  continuation: {
    beforeCursor: string | null
    afterCursor: string | null
    hasBefore: boolean
    hasAfter: boolean
  }
  readWatermark: { value: string } | null
  coverage: { kind: 'complete' | 'partial' | 'unknown'; details?: unknown }
  knownTotalCount: number | null
  pendingMutations: string[]
  anchor: unknown
}

export type RuntimeViewFrame<TData = unknown> =
  | { kind: 'snapshot'; snapshot: RuntimeViewSnapshot<TData> }
  | { kind: 'replace'; snapshot: RuntimeViewSnapshot<TData> }
  | { kind: 'error'; viewId: string; revision: number; error: unknown }
  | { kind: 'closed'; viewId: string }

export type RuntimeFrame<TData = unknown> =
  | {
      type: 'viewSnapshot'
      sessionSeq: number
      viewId: string
      revision: number
      snapshot: RuntimeViewSnapshot<TData>
    }
  | {
      type: 'viewReplace'
      sessionSeq: number
      viewId: string
      revision: number
      snapshot: RuntimeViewSnapshot<TData>
    }
  | { type: 'viewError'; sessionSeq: number; viewId: string; error: unknown }
  | { type: 'viewClosed'; sessionSeq: number; viewId: string }
  | {
      type: 'mutationSettlement'
      sessionSeq: number
      mutationId: string
      state: unknown
    }
  | { type: 'notification'; sessionSeq: number; kind: string; payload: unknown }
  | { type: 'heartbeat'; sessionSeq: number }

export interface RuntimeSession {
  sessionId: string
}

export interface RuntimeOpenSessionRequest {
  sourceId?: string | null
}

export interface RuntimeCloseSessionRequest {
  sessionId: string
  sourceId?: string | null
}

export interface RuntimeSessionViewRequest {
  sessionId: string
  view: RuntimeMessagePageRequest
}

export interface RuntimeOpenMessageListViewResult {
  viewId: string
  snapshot: RuntimeViewSnapshot<RuntimeMailListViewState>
}

export interface RuntimeViewSubscriptionRequest {
  viewId: string
  afterRevision?: number | null
  sourceId?: string | null
}

export interface RuntimeFrameSubscriptionRequest {
  sessionId: string
  afterSeq?: number | null
  sourceId?: string | null
}

export interface RuntimeViewFrameHandlers {
  onFrame(frame: RuntimeViewFrame<RuntimeMailListViewState>): void
  onMalformedFrame?(input: { raw: string; error: unknown }): void
  onPermanentError?(error: unknown): void
  onTransientError?(error: unknown): void
  onClosed?(error: unknown): void
}

export interface RuntimeFrameHandlers {
  onFrame(frame: RuntimeFrame<RuntimeMailListViewState>): void
  onMalformedFrame?(input: { raw: string; error: unknown }): void
  onPermanentError?(error: unknown): void
  onTransientError?(error: unknown): void
  onClosed?(error: unknown): void
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

export interface RuntimeMoveMessageToMailboxRoleRequest {
  sourceId: string
  messageId: string
  role: KnownMailboxRole
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
  openRuntimeSession(
    request: RuntimeOpenSessionRequest,
  ): Promise<RuntimeSession>
  closeRuntimeSession(request: RuntimeCloseSessionRequest): Promise<OkResponse>
  openRuntimeSessionMessageListView(
    request: RuntimeSessionViewRequest,
  ): Promise<RuntimeOpenMessageListViewResult>
  subscribeRuntimeFrames(
    request: RuntimeFrameSubscriptionRequest,
    handlers: RuntimeFrameHandlers,
  ): RuntimeUnsubscribe
  openMessageListView(
    request: RuntimeMessagePageRequest,
  ): Promise<RuntimeOpenMessageListViewResult>
  subscribeView(
    request: RuntimeViewSubscriptionRequest,
    handlers: RuntimeViewFrameHandlers,
  ): RuntimeUnsubscribe
  createAccount(input: CreateAccountInput): Promise<AccountOverview>
  createSmartMailbox(input: CreateSmartMailboxInput): Promise<SmartMailbox>
  deleteAccount(accountId: string): Promise<OkResponse>
  deleteSmartMailbox(smartMailboxId: string): Promise<OkResponse>
  disableAccount(accountId: string): Promise<OkResponse>
  enableAccount(accountId: string): Promise<OkResponse>
  fetchAccount(accountId: string): Promise<AccountOverview>
  fetchAccounts(): Promise<AccountOverview[]>
  fetchConversation(conversationId: string): Promise<ConversationView>
  fetchConversationPage(
    request?: RuntimeConversationPageRequest,
  ): Promise<ConversationPage>
  fetchIdentity(sourceId: string): Promise<Identity>
  fetchMailboxes(accountId: string): Promise<Mailbox[]>
  fetchMessage(messageId: string, sourceId: string): Promise<MessageDetail>
  fetchMessagePage(request: RuntimeMessagePageRequest): Promise<MessagePage>
  fetchOAuthRedirectUri(): string
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
  moveMessageToMailboxRole(
    request: RuntimeMoveMessageToMailboxRoleRequest,
  ): Promise<MessageCommandResult>
  sendMessage(request: RuntimeSendMessageRequest): Promise<OkResponse>
  startProviderOAuth(
    input: StartProviderOAuthInput,
  ): Promise<StartOAuthResponse>
  updateAccount(
    accountId: string,
    input: UpdateAccountInput,
  ): Promise<AccountOverview>
  uploadAccountLogo(accountId: string, file: File): Promise<AccountOverview>
  verifyAccount(accountId: string): Promise<VerificationResponse>
  updateSmartMailbox(
    smartMailboxId: string,
    input: UpdateSmartMailboxInput,
  ): Promise<SmartMailbox>
  triggerSync(
    request: RuntimeTriggerSyncRequest,
  ): Promise<RuntimeTriggerSyncResult>
}
