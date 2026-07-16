import type {
  AccountOverview,
  ConversationView,
  CreateAccountInput,
  Mailbox,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  OkResponse,
  ReadRequest,
  ReadResponse,
  SmartMailboxSummary,
  StartOAuthResponse,
  StartProviderOAuthInput,
  UpdateAccountInput,
  VerificationResponse,
} from '../api/types'

import type {
  RuntimeAdapter,
  RuntimeCloseLinkRequest,
  RuntimeFrame,
  RuntimeFrameSubscriptionRequest,
  RuntimeMailListViewState,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
  RuntimeMoveMessageToMailboxRoleRequest,
  RuntimeMutationReceipt,
  RuntimeOpenMessageListViewResult,
  RuntimeOpenViewResult,
  RuntimeOpenLinkRequest,
  RuntimeResourceDescriptor,
  RuntimeRunMutationRequest,
  RuntimeLinkConnection,
  RuntimeLinkObjectViewRequest,
  RuntimeLinkViewCloseRequest,
  RuntimeLinkViewExtendRequest,
  RuntimeLinkViewRequest,
  RuntimeTriggerSyncRequest,
  RuntimeTriggerSyncResult,
} from './types'

export type AccountCommandCall = {
  kind: 'enable' | 'disable' | 'delete'
  accountId: string
}
export type AccountLogoUploadCall = { accountId: string; file: File }
export type AccountUpdateCall = {
  accountId: string
  input: UpdateAccountInput
}
export type RuntimeFrameSubscriptionCall = {
  request: RuntimeFrameSubscriptionRequest
}
export type RuntimeMutationCall = {
  request: RuntimeRunMutationRequest
}
export type OAuthStartCall = Omit<StartProviderOAuthInput, 'clientSecret'> & {
  hasClientSecret: boolean
}
export type MessageDetailCall = { messageId: string; sourceId: string }
export type ResourceCall = { descriptor: RuntimeResourceDescriptor }

export type QueuedOutcome<T> =
  | { kind: 'resolve'; value: T }
  | { kind: 'reject'; error: Error }

export interface FakeRuntimeAdapter extends RuntimeAdapter {
  readonly accountCalls: number
  readonly accountCommandCalls: AccountCommandCall[]
  readonly accountCreateCalls: CreateAccountInput[]
  readonly accountDetailCalls: string[]
  readonly accountLogoUploadCalls: AccountLogoUploadCall[]
  readonly accountUpdateCalls: AccountUpdateCall[]
  readonly accountVerificationCalls: string[]
  readonly conversationCalls: string[]
  readonly runtimeLinkCalls: RuntimeOpenLinkRequest[]
  readonly runtimeLinkCloseCalls: RuntimeCloseLinkRequest[]
  readonly runtimeLinkViewOpenCalls: RuntimeLinkViewRequest[]
  readonly runtimeLinkObjectViewOpenCalls: RuntimeLinkObjectViewRequest[]
  readonly runtimeLinkViewExtendCalls: RuntimeLinkViewExtendRequest[]
  readonly runtimeLinkViewCloseCalls: RuntimeLinkViewCloseRequest[]
  readonly runtimeFrameSubscriptionCalls: RuntimeFrameSubscriptionCall[]
  readonly runtimeMutationCalls: RuntimeMutationCall[]
  readonly mailboxCalls: string[]
  readonly messageCalls: MessageDetailCall[]
  readonly messageCommandCalls: RuntimeMessageCommandRequest[]
  readonly messageRoleMoveCalls: RuntimeMoveMessageToMailboxRoleRequest[]
  readonly messagePageCalls: RuntimeMessagePageRequest[]
  readonly oauthStartCalls: OAuthStartCall[]
  readonly readCalls: ReadRequest[]
  readonly resourceCalls: ResourceCall[]
  readonly smartMailboxCalls: number
  readonly syncCalls: RuntimeTriggerSyncRequest[]
  emitRuntimeFrame(frame: RuntimeFrame<RuntimeMailListViewState>): void
  /** Simulate a hard close of the runtime frame stream (fires `onClosed`). */
  emitRuntimeFrameStreamClosed(error?: unknown): void
  queueAccount(account: AccountOverview): void
  queueAccountError(error: Error): void
  queueAccountOk(result: OkResponse): void
  queueAccountOkError(error: Error): void
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
  queueRuntimeLinkConnection(link: RuntimeLinkConnection): void
  queueRuntimeLinkError(error: Error): void
  queueRuntimeLinkMessageListView(
    result: RuntimeOpenMessageListViewResult,
  ): void
  queueRuntimeLinkMessageListViewError(error: Error): void
  queueRuntimeLinkView(result: RuntimeOpenViewResult): void
  queueRuntimeLinkViewError(error: Error): void
  queueRuntimeLinkViewExtend(result: RuntimeOpenMessageListViewResult): void
  queueRuntimeLinkViewExtendError(error: Error): void
  queueRuntimeMutationReceipt(receipt: RuntimeMutationReceipt): void
  queueRuntimeMutationError(error: Error): void
  queueOAuthStartResponse(response: StartOAuthResponse): void
  queueOAuthStartError(error: Error): void
  queueResourceBlob(blob: Blob): void
  queueResourceError(error: Error): void
  queueReadResponse(response: ReadResponse): void
  queueReadError(error: Error): void
  queueSmartMailboxes(mailboxes: SmartMailboxSummary[]): void
  queueSmartMailboxesError(error: Error): void
  queueSyncResult(result: RuntimeTriggerSyncResult): void
  queueVerificationResponse(response: VerificationResponse): void
  queueVerificationError(error: Error): void
  queueSyncError(error: Error): void
  reset(): void
}

export type FakeCallRecords = {
  accountCommandCalls: AccountCommandCall[]
  accountCreateCalls: CreateAccountInput[]
  accountDetailCalls: string[]
  accountLogoUploadCalls: AccountLogoUploadCall[]
  accountUpdateCalls: AccountUpdateCall[]
  accountVerificationCalls: string[]
  conversationCalls: string[]
  runtimeLinkCalls: RuntimeOpenLinkRequest[]
  runtimeLinkCloseCalls: RuntimeCloseLinkRequest[]
  runtimeLinkViewOpenCalls: RuntimeLinkViewRequest[]
  runtimeLinkObjectViewOpenCalls: RuntimeLinkObjectViewRequest[]
  runtimeLinkViewExtendCalls: RuntimeLinkViewExtendRequest[]
  runtimeLinkViewCloseCalls: RuntimeLinkViewCloseRequest[]
  runtimeFrameSubscriptionCalls: RuntimeFrameSubscriptionCall[]
  runtimeMutationCalls: RuntimeMutationCall[]
  mailboxCalls: string[]
  messageCalls: MessageDetailCall[]
  messageCommandCalls: RuntimeMessageCommandRequest[]
  messageRoleMoveCalls: RuntimeMoveMessageToMailboxRoleRequest[]
  messagePageCalls: RuntimeMessagePageRequest[]
  oauthStartCalls: OAuthStartCall[]
  readCalls: ReadRequest[]
  resourceCalls: ResourceCall[]
  syncCalls: RuntimeTriggerSyncRequest[]
}

export type FakeQueues = {
  accountOkResults: QueuedOutcome<OkResponse>[]
  accountResults: QueuedOutcome<AccountOverview>[]
  accounts: QueuedOutcome<AccountOverview[]>[]
  conversations: QueuedOutcome<ConversationView>[]
  mailboxes: QueuedOutcome<Mailbox[]>[]
  messages: QueuedOutcome<MessageDetail>[]
  messageCommands: QueuedOutcome<MessageCommandResult>[]
  messagePages: QueuedOutcome<MessagePage>[]
  runtimeLinks: QueuedOutcome<RuntimeLinkConnection>[]
  runtimeLinkMessageListViews: QueuedOutcome<RuntimeOpenMessageListViewResult>[]
  runtimeLinkViews: QueuedOutcome<RuntimeOpenViewResult>[]
  runtimeLinkViewExtends: QueuedOutcome<RuntimeOpenMessageListViewResult>[]
  runtimeMutations: QueuedOutcome<RuntimeMutationReceipt>[]
  oauthStartResponses: QueuedOutcome<StartOAuthResponse>[]
  reads: QueuedOutcome<ReadResponse>[]
  resources: QueuedOutcome<Blob>[]
  smartMailboxes: QueuedOutcome<SmartMailboxSummary[]>[]
  syncs: QueuedOutcome<RuntimeTriggerSyncResult>[]
  verificationResponses: QueuedOutcome<VerificationResponse>[]
}

export interface FakeRuntimeAdapterOptions {
  defaultAccount?: AccountOverview
  defaultAccountOk?: OkResponse
  defaultAccounts?: AccountOverview[]
  defaultConversation?: ConversationView
  defaultMailboxes?: Mailbox[]
  defaultMessage?: MessageDetail
  defaultMessageCommandResult?: MessageCommandResult
  defaultMessagePage?: MessagePage
  defaultRuntimeLinkConnection?: RuntimeLinkConnection
  defaultRuntimeLinkMessageListView?: RuntimeOpenMessageListViewResult
  defaultRuntimeLinkView?: RuntimeOpenViewResult
  defaultRuntimeLinkViewExtend?: RuntimeOpenMessageListViewResult
  defaultRuntimeMutationReceipt?: RuntimeMutationReceipt
  defaultOAuthStartResponse?: StartOAuthResponse
  defaultReadResponse?: ReadResponse
  defaultSmartMailboxes?: SmartMailboxSummary[]
  defaultVerificationResponse?: VerificationResponse
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

export function createFakeCallRecords(): FakeCallRecords {
  return {
    accountCommandCalls: [],
    accountCreateCalls: [],
    accountDetailCalls: [],
    accountLogoUploadCalls: [],
    accountUpdateCalls: [],
    accountVerificationCalls: [],
    conversationCalls: [],
    runtimeLinkCalls: [],
    runtimeLinkCloseCalls: [],
    runtimeLinkViewOpenCalls: [],
    runtimeLinkObjectViewOpenCalls: [],
    runtimeLinkViewExtendCalls: [],
    runtimeLinkViewCloseCalls: [],
    runtimeFrameSubscriptionCalls: [],
    runtimeMutationCalls: [],
    mailboxCalls: [],
    messageCalls: [],
    messageCommandCalls: [],
    messageRoleMoveCalls: [],
    messagePageCalls: [],
    oauthStartCalls: [],
    readCalls: [],
    resourceCalls: [],
    syncCalls: [],
  }
}

export function resetFakeCallRecords(calls: FakeCallRecords): void {
  calls.accountCommandCalls.length = 0
  calls.accountCreateCalls.length = 0
  calls.accountDetailCalls.length = 0
  calls.accountLogoUploadCalls.length = 0
  calls.accountUpdateCalls.length = 0
  calls.accountVerificationCalls.length = 0
  calls.conversationCalls.length = 0
  calls.runtimeLinkCalls.length = 0
  calls.runtimeLinkCloseCalls.length = 0
  calls.runtimeLinkViewOpenCalls.length = 0
  calls.runtimeLinkObjectViewOpenCalls.length = 0
  calls.runtimeLinkViewExtendCalls.length = 0
  calls.runtimeLinkViewCloseCalls.length = 0
  calls.runtimeFrameSubscriptionCalls.length = 0
  calls.runtimeMutationCalls.length = 0
  calls.mailboxCalls.length = 0
  calls.messageCalls.length = 0
  calls.messageCommandCalls.length = 0
  calls.messageRoleMoveCalls.length = 0
  calls.messagePageCalls.length = 0
  calls.oauthStartCalls.length = 0
  calls.readCalls.length = 0
  calls.resourceCalls.length = 0
  calls.syncCalls.length = 0
}

export function createFakeQueues(): FakeQueues {
  return {
    accountOkResults: [],
    accountResults: [],
    accounts: [],
    conversations: [],
    mailboxes: [],
    messages: [],
    messageCommands: [],
    messagePages: [],
    runtimeLinks: [],
    runtimeLinkMessageListViews: [],
    runtimeLinkViews: [],
    runtimeLinkViewExtends: [],
    runtimeMutations: [],
    oauthStartResponses: [],
    reads: [],
    resources: [],
    smartMailboxes: [],
    syncs: [],
    verificationResponses: [],
  }
}

export function resetFakeQueues(queues: FakeQueues): void {
  queues.accountOkResults.length = 0
  queues.accountResults.length = 0
  queues.accounts.length = 0
  queues.conversations.length = 0
  queues.mailboxes.length = 0
  queues.messages.length = 0
  queues.messageCommands.length = 0
  queues.messagePages.length = 0
  queues.runtimeLinks.length = 0
  queues.runtimeLinkMessageListViews.length = 0
  queues.runtimeLinkViews.length = 0
  queues.runtimeLinkViewExtends.length = 0
  queues.runtimeMutations.length = 0
  queues.oauthStartResponses.length = 0
  queues.reads.length = 0
  queues.resources.length = 0
  queues.smartMailboxes.length = 0
  queues.syncs.length = 0
  queues.verificationResponses.length = 0
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

export function unsupported<T>(label: string): Promise<T> {
  return Promise.reject(new Error(`fake runtime adapter has no ${label}`))
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
  return unsupported(label)
}
