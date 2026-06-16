import type { MessageCommandResult } from '../api/types'

import { httpRuntimeAdapter } from './httpAdapter'
import type { RuntimeAdapter, RuntimeMessageCommandRequest } from './types'

let activeRuntimeAdapter: RuntimeAdapter = httpRuntimeAdapter

/** Current renderer runtime adapter. Seeded to the HTTP bridge for compatibility. */
export function getRuntimeAdapter(): RuntimeAdapter {
  return activeRuntimeAdapter
}

/** Dispatch a message command through the active runtime adapter. */
export function runRuntimeMessageCommand(
  request: RuntimeMessageCommandRequest,
): Promise<MessageCommandResult> {
  return activeRuntimeAdapter.runMessageCommand(request)
}

/** Test-only: override the active adapter without starting a backend. */
export function setRuntimeAdapterForTesting(
  adapter: RuntimeAdapter,
): () => void {
  activeRuntimeAdapter = adapter
  return resetRuntimeAdapterForTesting
}

/** Test-only: restore the production-compatible HTTP adapter. */
export function resetRuntimeAdapterForTesting(): void {
  activeRuntimeAdapter = httpRuntimeAdapter
}
