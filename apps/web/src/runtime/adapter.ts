import {
  injectedRuntimeMode,
  type InjectedRuntimeMode,
} from '../connection/injected'
import { LOG_EVENTS, syncLogger } from '../logger'

import { httpRuntimeAdapter } from './httpAdapter'
import { loadEntityStoreHandleFactory } from './replica/handle'
import { defaultPendingSetStore } from './replica/pendingSetStore'
import { createEntityStoreAdapter } from './replica/entityStoreAdapter'
import { createWorkerStorePort } from './replica/workerStorePort'
import { resolveStorePort } from './replica/storePortResolver'
import { installUnloadDurabilityHooks } from './replica/unloadDurability'
import type { RuntimeAdapter } from './types'

function unsupportedRuntimeAdapter(mode: InjectedRuntimeMode): RuntimeAdapter {
  const reject = <T>(): Promise<T> =>
    Promise.reject(new Error(`runtime adapter mode ${mode} is not implemented`))
  return {
    openRuntimeLink: () => reject(),
    closeRuntimeLink: () => reject(),
    openRuntimeLinkMessageListView: () => reject(),
    openRuntimeLinkView: () => reject(),
    extendRuntimeLinkView: () => reject(),
    closeRuntimeLinkView: () => reject(),
    runRuntimeMutation: () => reject(),
    subscribeRuntimeFrames: (_request, handlers) => {
      handlers.onPermanentError?.(
        new Error(`runtime adapter mode ${mode} is not implemented`),
      )
      return () => undefined
    },
    createAccount: () => reject(),
    createRule: () => reject(),
    createSmartMailbox: () => reject(),
    deleteAccount: () => reject(),
    deleteRule: () => reject(),
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
    fetchRules: () => reject(),
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
    updateRule: () => reject(),
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

syncLogger.info(
  {
    event: LOG_EVENTS.runtimeAdapterInitialized,
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
    // The WASM entity store runs on a Web Worker by default — the worker keeps
    // the UI thread responsive during a re-sync burst (validated by
    // apps/web/e2e/worker-burst-probe.mjs: a 20k-event burst stays at 17ms max
    // rAF gap vs 50ms jank in-process). Set `VITE_REPLICA_WORKER=false` to force
    // the in-process store.
    //
    // Worker mode is PROBED before use: a webview that can't run the worker
    // (module-worker/WASM-in-worker/asset-resolution issues — the unvalidated
    // risk on the Tauri WKWebView/WebView2 targets) is detected via the worker's
    // readiness handshake and falls back to the in-process store, so defaulting
    // it on can't break the mail list anywhere.
    const store = await resolveStorePort({
      workerEnabled: import.meta.env?.VITE_REPLICA_WORKER !== 'false',
      createWorkerStorePort,
      loadHandle: loadEntityStoreHandleFactory,
      onFallback: (error) =>
        syncLogger.warn(
          {
            event: LOG_EVENTS.runtimeReplicaAdapterInstalled,
            store: 'in-process',
            reason: 'worker-unavailable',
            error: error instanceof Error ? error.message : String(error),
          },
          'replica worker unavailable; falling back to the in-process store',
        ),
    })
    activeRuntimeAdapter = createEntityStoreAdapter({
      base: activeRuntimeAdapter,
      makeStore: () => store.port,
      pendingSet: defaultPendingSetStore(),
    })
    syncLogger.info(
      {
        event: LOG_EVENTS.runtimeReplicaAdapterInstalled,
        entityStore: true,
        adapterMode: injectedRuntimeMode() ?? 'loopback',
        store: store.kind,
      },
      `entity-store adapter installed (${store.kind} store)`,
    )
  })()
  return entityStoreInstall
}

// The client-layer WASM entity store is the sole mail-list derivation (rows +
// counts on one stream) and the read model the runtime no longer re-serves
// redundantly (option iii). It is unconditional and has NO REST fallback: WASM
// load is validated across targets, so a failure is an anomaly to surface, not a
// mode to silently degrade into. On failure the base transport still renders
// from view frames, but without the store's optimism + count ownership — so the
// failure is logged at error level rather than swallowed. Until the WASM finishes
// loading the base HTTP adapter serves (bootstrap only).
void installEntityStoreAdapter().catch((error) => {
  syncLogger.error(
    {
      event: LOG_EVENTS.runtimeReplicaAdapterInstalled,
      error: error instanceof Error ? error.message : String(error),
    },
    'entity store failed to load — no REST fallback; the mail list will not update optimistically',
  )
})

// W3 / N18: flush any queued durable write on visibilitychange-hidden/pagehide
// so a tab close can't strand it mid-flight. Installed unconditionally (a
// no-op until `installEntityStoreAdapter` resolves, and a no-op outside a DOM
// environment) rather than chained after the entity store install, so it's
// also armed for the (rare, logged) case where the entity store never loads.
installUnloadDurabilityHooks()
