import {
  injectedRuntimeMode,
  type InjectedRuntimeMode,
} from '../connection/injected'
import { LOG_EVENTS, syncLogger } from '../logger'

import { httpRuntimeAdapter } from './httpAdapter'
import { loadEntityStoreHandleFactory } from './replica/handle'
import { defaultOutboxStore } from './replica/outboxStore'
import { markEntityStoreActive } from './entityStoreState'
import { createEntityStoreAdapter } from './replica/entityStoreAdapter'
import type { RuntimeAdapter } from './types'

function unsupportedRuntimeAdapter(mode: InjectedRuntimeMode): RuntimeAdapter {
  const reject = <T>(): Promise<T> =>
    Promise.reject(new Error(`runtime adapter mode ${mode} is not implemented`))
  return {
    openRuntimeSession: () => reject(),
    closeRuntimeSession: () => reject(),
    openRuntimeSessionMessageListView: () => reject(),
    openRuntimeSessionView: () => reject(),
    extendRuntimeSessionView: () => reject(),
    closeRuntimeSessionView: () => reject(),
    runRuntimeMutation: () => reject(),
    subscribeRuntimeFrames: (_request, handlers) => {
      handlers.onPermanentError?.(
        new Error(`runtime adapter mode ${mode} is not implemented`),
      )
      return () => undefined
    },
    openMessageListView: () => reject(),
    subscribeView: (_request, handlers) => {
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
    fetchDraftContent: () => reject(),
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
    moveMessageToMailboxRole: () => reject(),
    sendMessage: () => reject(),
    saveDraft: () => reject(),
    deleteDraft: () => reject(),
    listPendingOperations: () => reject(),
    discardOperation: () => reject(),
    retryOperation: () => reject(),
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

/** Whether the client-layer entity store is enabled (controlled by
 * VITE_ENTITY_STORE). */
export function entityStoreAdapterEnabled(): boolean {
  return import.meta.env?.VITE_ENTITY_STORE === 'true'
}

syncLogger.info(
  {
    event: LOG_EVENTS.runtimeAdapterInitialized,
    entityStoreEnabled: entityStoreAdapterEnabled(),
    adapterMode: injectedRuntimeMode() ?? 'loopback',
  },
  'runtime adapter initialized',
)

let entityStoreInstall: Promise<void> | undefined

/**
 * Load the WASM entity store, wrap the active adapter with the
 * entityStoreAdapter, and make it the active runtime adapter. Idempotent; the
 * renderer keeps using the base adapter until the WASM finishes loading. The
 * store is the single derivation for the mail list (rows + counts on one
 * stream); the legacy REST invalidation path retires in 2e.3.
 */
export function installEntityStoreAdapter(): Promise<void> {
  entityStoreInstall ??= (async () => {
    const makeHandle = await loadEntityStoreHandleFactory()
    activeRuntimeAdapter = createEntityStoreAdapter({
      base: activeRuntimeAdapter,
      makeHandle,
      outbox: defaultOutboxStore(),
    })
    markEntityStoreActive()
    syncLogger.info(
      {
        event: LOG_EVENTS.runtimeReplicaAdapterInstalled,
        entityStore: true,
        adapterMode: injectedRuntimeMode() ?? 'loopback',
      },
      'entity-store adapter installed and active',
    )
  })()
  return entityStoreInstall
}

if (entityStoreAdapterEnabled()) {
  void installEntityStoreAdapter()
}
