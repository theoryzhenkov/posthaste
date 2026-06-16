import type {
  Mailbox,
  MessageCommandResult,
  MessagePage,
  ReadRequest,
  ReadResponse,
  SmartMailboxSummary,
} from '../api/types'

import { httpRuntimeAdapter } from './httpAdapter'
import type {
  RuntimeAdapter,
  RuntimeMessageCommandRequest,
  RuntimeMessagePageRequest,
} from './types'

let activeRuntimeAdapter: RuntimeAdapter = httpRuntimeAdapter

/** Current renderer runtime adapter. Seeded to the HTTP bridge for compatibility. */
export function getRuntimeAdapter(): RuntimeAdapter {
  return activeRuntimeAdapter
}

/** Execute a typed read call through the active runtime adapter. */
export function runtimeRead(request: ReadRequest): Promise<ReadResponse> {
  return activeRuntimeAdapter.read(request)
}

/** Fetch source mailboxes through the active runtime adapter. */
export function fetchRuntimeMailboxes(accountId: string): Promise<Mailbox[]> {
  return activeRuntimeAdapter.fetchMailboxes(accountId)
}

/** Fetch saved smart mailboxes through the active runtime adapter. */
export function fetchRuntimeSmartMailboxes(): Promise<SmartMailboxSummary[]> {
  return activeRuntimeAdapter.fetchSmartMailboxes()
}

/** Fetch a message page through the active runtime adapter. */
export function fetchRuntimeMessagePage(
  request: RuntimeMessagePageRequest,
): Promise<MessagePage> {
  return activeRuntimeAdapter.fetchMessagePage(request)
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
