import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CachedSenderAddress,
  ConversationPage,
  ConversationView,
  CreateAccountInput,
  CreateSmartMailboxInput,
  DraftContent,
  Mailbox,
  MessageCommand,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  MessageSortField,
  MessageSummary,
  OkResponse,
  Operation,
  PatchMailboxInput,
  PatchSettingsInput,
  ReadRequest,
  ReplyContext,
  ReadResponse,
  Rule,
  SaveDraftInput,
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
  WritableRuleInput,
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
  clientMutationId?: string | null
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

/** A view descriptor: the family + payload, plus the `clientSelfMaintained` flag
 * the client stamps for mail-list views it self-maintains (evaluable
 * predicate). See `isMailListSelfMaintained`. */
export interface RuntimeViewDescriptor {
  family: string
  payload: unknown
  clientSelfMaintained?: boolean
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
  descriptor: RuntimeViewDescriptor
  revision: number
  lifecycle: 'loading' | 'ready' | 'updating' | 'error'
  readWatermark: { value: string } | null
  coverage: { ranges?: { from?: unknown; to?: unknown }[] }
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
  coverage: { ranges?: { from?: unknown; to?: unknown }[] }
  knownTotalCount: number | null
  pendingMutations: string[]
  anchor: unknown
}

/**
 * An incremental mail-list update (replication client-link): only the rows that changed.
 * Reconcile against the held rows — when `order` is present, drop rows whose key
 * is absent and reorder to it; then apply `upserts` by `rowKey`.
 */
export interface RuntimeMailListDelta {
  order: string[] | null
  upserts: RuntimeMailListRowState[]
}

export type RuntimeViewFrame<TData = unknown> =
  | { kind: 'snapshot'; snapshot: RuntimeViewSnapshot<TData> }
  | { kind: 'replace'; snapshot: RuntimeViewSnapshot<TData> }
  | { kind: 'error'; viewId: string; revision: number; error: unknown }
  | { kind: 'closed'; viewId: string }

export type RuntimeMutationSettlementStatus =
  | 'accepted'
  | 'localApplied'
  | 'queued'
  | 'confirmed'
  | 'failed'
  | 'conflict'

/**
 * The typed retryability verdict (RFC-L2 D70), replacing the write-only
 * `retryable: boolean`. `'transient'` = a retry may succeed; `'permanent'` =
 * retrying is futile. The reason travels in the paired `code`.
 */
export type Terminality = 'transient' | 'permanent'

export interface RuntimeAdapterError {
  code: string
  message: string
  terminality: Terminality
  correlationId?: string | null
  details?: unknown
}

/**
 * A terminal verdict about a named client mutation, carried by the
 * `mutationNotification` frame and keyed to the client mutation id. `confirmed`
 * retires the optimistic op by absorption (when the base carries the effect);
 * `rejected` reverts it and surfaces the error. The non-terminal acks are not
 * sent — the client tracks the in-flight op in its own pending set.
 */
export type RuntimeMutationNotification =
  | { type: 'confirmed' }
  | { type: 'rejected'; error: RuntimeAdapterError }

export interface RuntimeMutationReceipt {
  runtimeMutationId: string | null
  clientMutationId: string
  name: string
  state: RuntimeMutationSettlementStatus
  error: RuntimeAdapterError | null
  output?: unknown
}

export type RuntimeFrame<TData = unknown> =
  | {
      type: 'viewSnapshot'
      linkSeq: number
      viewId: string
      revision: number
      snapshot: RuntimeViewSnapshot<TData>
    }
  | {
      type: 'viewReplace'
      linkSeq: number
      viewId: string
      revision: number
      snapshot: RuntimeViewSnapshot<TData>
    }
  | {
      type: 'viewDelta'
      linkSeq: number
      viewId: string
      revision: number
      delta: RuntimeMailListDelta
    }
  | { type: 'viewError'; linkSeq: number; viewId: string; error: unknown }
  | { type: 'viewClosed'; linkSeq: number; viewId: string }
  | {
      type: 'mutationNotification'
      linkSeq: number
      clientMutationId: string
      notification: RuntimeMutationNotification
    }
  | { type: 'notification'; linkSeq: number; kind: string; payload: unknown }
  | { type: 'heartbeat'; linkSeq: number }

export interface RuntimeLinkConnection {
  linkId: string
}

export interface RuntimeOpenLinkRequest {
  sourceId?: string | null
  /** Opt into incremental mail-list view deltas (replication client-link). */
  viewDelta?: boolean
}

export interface RuntimeCloseLinkRequest {
  linkId: string
  sourceId?: string | null
}

export interface RuntimeLinkViewRequest {
  linkId: string
  view: RuntimeMessagePageRequest
  sourceId?: string | null
}

export interface RuntimeLinkViewCloseRequest {
  linkId: string
  viewId: string
  sourceId?: string | null
}

export interface RuntimeLinkViewExtendRequest {
  linkId: string
  viewId: string
  count: number
  sourceId?: string | null
}

/// Open any runtime view family by descriptor (messageDetail, conversation, …).
/// `openRuntimeLinkMessageListView` stays specialized for the typed mail-list
/// page result; this is the generic single-object path.
export interface RuntimeLinkObjectViewRequest {
  linkId: string
  descriptor: RuntimeViewDescriptor
  sourceId?: string | null
}

export interface RuntimeRunMutationRequest {
  linkId?: string | null
  name: string
  args?: unknown
  clientMutationId: string
  context?: unknown
  sourceId?: string | null
}

export interface RuntimeOpenMessageListViewResult {
  viewId: string
  snapshot: RuntimeViewSnapshot<RuntimeMailListViewState>
}

export interface RuntimeOpenViewResult<TData = unknown> {
  viewId: string
  snapshot: RuntimeViewSnapshot<TData>
}

export interface RuntimeFrameSubscriptionRequest {
  linkId: string
  afterSeq?: number | null
  sourceId?: string | null
}

export interface RuntimeFrameHandlers {
  onFrame(frame: RuntimeFrame<RuntimeMailListViewState>): void
  onMalformedFrame?(input: { raw: string; error: unknown }): void
  onPermanentError?(error: unknown): void
  onTransientError?(error: unknown): void
  /**
   * The near node's incremental view is broken and must be rebuilt from scratch
   * (D49): a detected seq gap the far-end could not replay. Drop stale
   * incremental state; the runtime re-serves whole snapshots that re-seed it.
   */
  onReset?(): void
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
  | {
      kind: 'message-body'
      sourceId: string
      messageId: string
      format: 'html' | 'text'
    }

export interface RuntimeResourceFetchOptions {
  signal?: AbortSignal
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

export interface RuntimeSaveDraftRequest {
  sourceId: string
  input: SaveDraftInput
}

export interface RuntimeDeleteDraftRequest {
  sourceId: string
  draftId: string
}

export interface RuntimeMoveMessageToMailboxRoleRequest {
  sourceId: string
  messageId: string
  role: KnownMailboxRole
}

export type RuntimeUnsubscribe = () => void

/** Renderer-facing runtime adapter facade. */
export interface RuntimeAdapter {
  openRuntimeLink(
    request: RuntimeOpenLinkRequest,
  ): Promise<RuntimeLinkConnection>
  closeRuntimeLink(request: RuntimeCloseLinkRequest): Promise<OkResponse>
  openRuntimeLinkMessageListView(
    request: RuntimeLinkViewRequest,
  ): Promise<RuntimeOpenMessageListViewResult>
  openRuntimeLinkView(
    request: RuntimeLinkObjectViewRequest,
  ): Promise<RuntimeOpenViewResult>
  extendRuntimeLinkView(
    request: RuntimeLinkViewExtendRequest,
  ): Promise<RuntimeOpenMessageListViewResult>
  closeRuntimeLinkView(
    request: RuntimeLinkViewCloseRequest,
  ): Promise<OkResponse>
  runRuntimeMutation(
    request: RuntimeRunMutationRequest,
  ): Promise<RuntimeMutationReceipt>
  subscribeRuntimeFrames(
    request: RuntimeFrameSubscriptionRequest,
    handlers: RuntimeFrameHandlers,
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
  fetchDraftContent(request: RuntimeReplyContextRequest): Promise<DraftContent>
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
  patchSettings(input: PatchSettingsInput): Promise<AppSettings>
  previewAutomationRule(
    input: AutomationRulePreviewInput,
  ): Promise<AutomationRulePreviewResponse>
  read(request: ReadRequest): Promise<ReadResponse>
  fetchRules(): Promise<Rule[]>
  createRule(input: WritableRuleInput): Promise<Rule>
  updateRule(id: string, input: WritableRuleInput): Promise<Rule>
  deleteRule(id: string): Promise<void>
  resetDefaultSmartMailboxes(): Promise<SmartMailboxSummary[]>
  runMessageCommand(
    request: RuntimeMessageCommandRequest,
  ): Promise<MessageCommandResult>
  moveMessageToMailboxRole(
    request: RuntimeMoveMessageToMailboxRoleRequest,
  ): Promise<MessageCommandResult>
  sendMessage(request: RuntimeSendMessageRequest): Promise<OkResponse>
  saveDraft(request: RuntimeSaveDraftRequest): Promise<Operation>
  deleteDraft(request: RuntimeDeleteDraftRequest): Promise<Operation>
  listPendingOperations(sourceId: string): Promise<Operation[]>
  discardOperation(sourceId: string, operationId: string): Promise<void>
  retryOperation(sourceId: string, operationId: string): Promise<void>
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
