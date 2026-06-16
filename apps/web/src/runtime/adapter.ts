import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CachedSenderAddress,
  ConversationPage,
  ConversationView,
  CreateSmartMailboxInput,
  Mailbox,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  OkResponse,
  PatchMailboxInput,
  ReadRequest,
  ReplyContext,
  ReadResponse,
  SendMessageInput,
  SmartMailbox,
  SmartMailboxSummary,
  Identity,
  UpdateSmartMailboxInput,
} from '../api/types'
import {
  injectedRuntimeMode,
  type InjectedRuntimeMode,
} from '../connection/injected'

import { httpRuntimeAdapter } from './httpAdapter'
import type {
  RuntimeAdapter,
  RuntimeConversationPageRequest,
  RuntimeEventHandlers,
  RuntimeEventSubscriptionRequest,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
  RuntimeReplyContextRequest,
  RuntimeResourceDescriptor,
  RuntimeResourceFetchOptions,
  RuntimeTriggerSyncRequest,
  RuntimeTriggerSyncResult,
  RuntimeUnsubscribe,
} from './types'

function unsupportedRuntimeAdapter(mode: InjectedRuntimeMode): RuntimeAdapter {
  const reject = <T>(): Promise<T> =>
    Promise.reject(new Error(`runtime adapter mode ${mode} is not implemented`))
  return {
    subscribeEvents: (_request, handlers) => {
      handlers.onPermanentError?.(
        new Error(`runtime adapter mode ${mode} is not implemented`),
      )
      return () => undefined
    },
    createAccount: () => reject(),
    createSmartMailbox: () => reject(),
    deleteAccount: () => reject(),
    deleteSmartMailbox: () => reject(),
    disableAccount: () => reject(),
    enableAccount: () => reject(),
    fetchAccount: () => reject(),
    fetchAccounts: () => reject(),
    fetchConversation: () => reject(),
    fetchConversationPage: () => reject(),
    fetchIdentity: () => reject(),
    fetchMailboxes: () => reject(),
    fetchMessage: () => reject(),
    fetchMessagePage: () => reject(),
    fetchOAuthRedirectUri: () => {
      throw new Error(`runtime adapter mode ${mode} is not implemented`)
    },
    fetchReplyContext: () => reject(),
    fetchResourceBlob: () => reject(),
    fetchSenderAddresses: () => reject(),
    fetchSettings: () => reject(),
    fetchSmartMailbox: () => reject(),
    fetchSmartMailboxes: () => reject(),
    patchMailbox: () => reject(),
    patchSettings: () => reject(),
    previewAutomationRule: () => reject(),
    read: () => reject(),
    resetDefaultSmartMailboxes: () => reject(),
    runMessageCommand: () => reject(),
    sendMessage: () => reject(),
    startProviderOAuth: () => reject(),
    triggerSync: () => reject(),
    updateAccount: () => reject(),
    updateSmartMailbox: () => reject(),
    uploadAccountLogo: () => reject(),
    verifyAccount: () => reject(),
  }
}

export function runtimeAdapterForMode(
  mode: InjectedRuntimeMode | undefined,
): RuntimeAdapter {
  switch (mode) {
    case undefined:
    case 'loopback':
      return httpRuntimeAdapter
    case 'native':
      return unsupportedRuntimeAdapter(mode)
  }
}

function defaultRuntimeAdapter(): RuntimeAdapter {
  return runtimeAdapterForMode(injectedRuntimeMode())
}

let activeRuntimeAdapter: RuntimeAdapter = defaultRuntimeAdapter()

/** Current renderer runtime adapter. Seeded to the HTTP bridge for compatibility. */
export function getRuntimeAdapter(): RuntimeAdapter {
  return activeRuntimeAdapter
}

/** Subscribe to runtime domain events through the active runtime adapter. */
export function subscribeRuntimeEvents(
  request: RuntimeEventSubscriptionRequest,
  handlers: RuntimeEventHandlers,
): RuntimeUnsubscribe {
  return activeRuntimeAdapter.subscribeEvents(request, handlers)
}

/** Execute a typed read call through the active runtime adapter. */
export function runtimeRead(request: ReadRequest): Promise<ReadResponse> {
  return activeRuntimeAdapter.read(request)
}

/** Create a saved smart mailbox through the active runtime adapter. */
export function createRuntimeSmartMailbox(
  input: CreateSmartMailboxInput,
): Promise<SmartMailbox> {
  return activeRuntimeAdapter.createSmartMailbox(input)
}

/** Delete a saved smart mailbox through the active runtime adapter. */
export function deleteRuntimeSmartMailbox(
  smartMailboxId: string,
): Promise<OkResponse> {
  return activeRuntimeAdapter.deleteSmartMailbox(smartMailboxId)
}

/** Fetch accounts through the active runtime adapter. */
export function fetchRuntimeAccounts(): Promise<AccountOverview[]> {
  return activeRuntimeAdapter.fetchAccounts()
}

/** Fetch a conversation view through the active runtime adapter. */
export function fetchRuntimeConversation(
  conversationId: string,
): Promise<ConversationView> {
  return activeRuntimeAdapter.fetchConversation(conversationId)
}

/** Fetch a conversation page through the active runtime adapter. */
export function fetchRuntimeConversationPage(
  request?: RuntimeConversationPageRequest,
): Promise<ConversationPage> {
  return activeRuntimeAdapter.fetchConversationPage(request)
}

/** Fetch sender identity through the active runtime adapter. */
export function fetchRuntimeIdentity(sourceId: string): Promise<Identity> {
  return activeRuntimeAdapter.fetchIdentity(sourceId)
}

/** Fetch source mailboxes through the active runtime adapter. */
export function fetchRuntimeMailboxes(accountId: string): Promise<Mailbox[]> {
  return activeRuntimeAdapter.fetchMailboxes(accountId)
}

/** Fetch app settings through the active runtime adapter. */
export function fetchRuntimeSettings(): Promise<AppSettings> {
  return activeRuntimeAdapter.fetchSettings()
}

/** Fetch cached sender addresses through the active runtime adapter. */
export function fetchRuntimeSenderAddresses(): Promise<CachedSenderAddress[]> {
  return activeRuntimeAdapter.fetchSenderAddresses()
}

/** Fetch a saved smart mailbox through the active runtime adapter. */
export function fetchRuntimeSmartMailbox(
  smartMailboxId: string,
): Promise<SmartMailbox> {
  return activeRuntimeAdapter.fetchSmartMailbox(smartMailboxId)
}

/** Fetch saved smart mailboxes through the active runtime adapter. */
export function fetchRuntimeSmartMailboxes(): Promise<SmartMailboxSummary[]> {
  return activeRuntimeAdapter.fetchSmartMailboxes()
}

/** Patch a source mailbox through the active runtime adapter. */
export function patchRuntimeMailbox(
  accountId: string,
  mailboxId: string,
  input: PatchMailboxInput,
): Promise<Mailbox[]> {
  return activeRuntimeAdapter.patchMailbox(accountId, mailboxId, input)
}

/** Patch app settings through the active runtime adapter. */
export function patchRuntimeSettings(
  input: Partial<AppSettings>,
): Promise<AppSettings> {
  return activeRuntimeAdapter.patchSettings(input)
}

/** Preview an automation rule through the active runtime adapter. */
export function previewRuntimeAutomationRule(
  input: AutomationRulePreviewInput,
): Promise<AutomationRulePreviewResponse> {
  return activeRuntimeAdapter.previewAutomationRule(input)
}

/** Fetch full message detail through the active runtime adapter. */
export function fetchRuntimeMessage(
  messageId: string,
  sourceId: string,
): Promise<MessageDetail> {
  return activeRuntimeAdapter.fetchMessage(messageId, sourceId)
}

/** Fetch a message page through the active runtime adapter. */
export function fetchRuntimeMessagePage(
  request: RuntimeMessagePageRequest,
): Promise<MessagePage> {
  return activeRuntimeAdapter.fetchMessagePage(request)
}

/** Fetch reply context through the active runtime adapter. */
export function fetchRuntimeReplyContext(
  request: RuntimeReplyContextRequest,
): Promise<ReplyContext> {
  return activeRuntimeAdapter.fetchReplyContext(request)
}

/** Fetch runtime-owned resource bytes through the active runtime adapter. */
export function fetchRuntimeResourceBlob(
  descriptor: RuntimeResourceDescriptor,
  options?: RuntimeResourceFetchOptions,
): Promise<Blob> {
  return activeRuntimeAdapter.fetchResourceBlob(descriptor, options)
}

/** Dispatch a message command through the active runtime adapter. */
export function runRuntimeMessageCommand(
  request: RuntimeMessageCommandRequest,
): Promise<MessageCommandResult> {
  return activeRuntimeAdapter.runMessageCommand(request)
}

/** Reset default smart mailboxes through the active runtime adapter. */
export function resetRuntimeDefaultSmartMailboxes(): Promise<
  SmartMailboxSummary[]
> {
  return activeRuntimeAdapter.resetDefaultSmartMailboxes()
}

/** Send a composed message through the active runtime adapter. */
export function sendRuntimeMessage(request: {
  sourceId: string
  input: SendMessageInput
}): Promise<OkResponse> {
  return activeRuntimeAdapter.sendMessage(request)
}

/** Update a saved smart mailbox through the active runtime adapter. */
export function updateRuntimeSmartMailbox(
  smartMailboxId: string,
  input: UpdateSmartMailboxInput,
): Promise<SmartMailbox> {
  return activeRuntimeAdapter.updateSmartMailbox(smartMailboxId, input)
}

/** Trigger account sync through the active runtime adapter. */
export function triggerRuntimeSync(
  request: RuntimeTriggerSyncRequest,
): Promise<RuntimeTriggerSyncResult> {
  return activeRuntimeAdapter.triggerSync(request)
}

/** Test-only: override the active adapter without starting a backend. */
export function setRuntimeAdapterForTesting(
  adapter: RuntimeAdapter,
): () => void {
  const previous = activeRuntimeAdapter
  activeRuntimeAdapter = adapter
  return () => {
    activeRuntimeAdapter = previous
  }
}

/** Test-only: restore the production-compatible HTTP adapter. */
export function resetRuntimeAdapterForTesting(): void {
  activeRuntimeAdapter = httpRuntimeAdapter
}
