import {
  injectedRuntimeMode,
  type InjectedRuntimeMode,
} from '../connection/injected'

import { httpRuntimeAdapter } from './httpAdapter'
import type { RuntimeAdapter } from './types'

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
    openRuntimeSession: () => reject(),
    closeRuntimeSession: () => reject(),
    openRuntimeSessionMessageListView: () => reject(),
    openRuntimeSessionView: () => reject(),
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
