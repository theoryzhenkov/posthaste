import type {
  AccountOverview,
  ConversationView,
  Mailbox,
  MessageCommandResult,
  MessageDetail,
  MessagePage,
  ReadRequest,
  ReadResponse,
  SmartMailboxSummary,
} from '../api/types'
import {
  injectedRuntimeMode,
  type InjectedRuntimeMode,
} from '../connection/injected'

import { httpRuntimeAdapter } from './httpAdapter'
import type {
  RuntimeAdapter,
  RuntimeEventHandlers,
  RuntimeEventSubscriptionRequest,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
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
    fetchAccounts: () => reject(),
    fetchConversation: () => reject(),
    fetchMailboxes: () => reject(),
    fetchMessage: () => reject(),
    fetchMessagePage: () => reject(),
    fetchResourceBlob: () => reject(),
    fetchSmartMailboxes: () => reject(),
    read: () => reject(),
    runMessageCommand: () => reject(),
    triggerSync: () => reject(),
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

/** Fetch source mailboxes through the active runtime adapter. */
export function fetchRuntimeMailboxes(accountId: string): Promise<Mailbox[]> {
  return activeRuntimeAdapter.fetchMailboxes(accountId)
}

/** Fetch saved smart mailboxes through the active runtime adapter. */
export function fetchRuntimeSmartMailboxes(): Promise<SmartMailboxSummary[]> {
  return activeRuntimeAdapter.fetchSmartMailboxes()
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
