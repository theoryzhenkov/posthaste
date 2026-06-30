/**
 * Async boundary over the client WASM entity store.
 *
 * The `EntityStoreController` programs against this interface so the store can
 * run **in-process** on the main thread (today, via {@link InProcessStorePort})
 * or **in a Web Worker** (a future `WorkerStorePort`) without changing any
 * orchestration. Making it async up front is what lets the worker move be a
 * drop-in: the controller already awaits + serializes every store op.
 *
 * The methods mirror the narrow JSON-string surface of {@link EntityStoreHandle}
 * that the controller uses — values cross as JSON strings exactly as they do at
 * the WASM boundary, so a worker boundary is the same payload over `postMessage`.
 *
 * @spec docs/eph/DESIGN-L2-replica-worker-isolation
 */
import type { EntityStoreHandle, SettlementVerdict } from './handle'

export interface StorePort {
  /** Register a view: `{predicate, sortField, sortDirection, watermark?}`. */
  registerViewJson(viewId: string, argsJson: string): Promise<void>
  /** Replace a view's rows + watermark (a served snapshot / page / resync). */
  setViewRowsJson(
    viewId: string,
    rowsJson: string,
    watermarkJson: string,
  ): Promise<void>
  closeView(viewId: string): Promise<void>
  /** Apply an authoritative batch atomically. */
  ingestBatchJson(batchJson: string): Promise<void>
  /** Accept an optimistic mutation: `{mutationId, messageId, assertion}`. */
  acceptMutationJson(acceptJson: string): Promise<void>
  /** Settle a pending mutation; resolves `true` when a failure reverted. */
  settle(mutationId: string, outcome: SettlementVerdict): Promise<boolean>
  /** The invertible change-diff a mutation would produce, WITHOUT applying it. */
  captureMutationDiffJson(
    messageId: string,
    assertionJson: string,
  ): Promise<string>
  /** A mailbox's counts `{unreadCount, totalCount}` JSON, or `"null"`. */
  mailboxJson(mailboxId: string): Promise<string>
  /** A view's projected rows JSON, or `"null"`. */
  projectViewJson(viewId: string): Promise<string>
  /** Drain the dirty keys since the last drain. */
  drainDirtyJson(): Promise<string>
  /** Drain the ids of ops retired since the last drain (JSON string array). */
  drainRetiredJson(): Promise<string>
}

/**
 * `StorePort` over the synchronous in-thread WASM handle. Each call resolves
 * immediately; the async signature is purely to share the interface with the
 * worker implementation.
 */
export class InProcessStorePort implements StorePort {
  private readonly handle: EntityStoreHandle

  constructor(handle: EntityStoreHandle) {
    this.handle = handle
  }

  registerViewJson(viewId: string, argsJson: string): Promise<void> {
    this.handle.registerViewJson(viewId, argsJson)
    return Promise.resolve()
  }

  setViewRowsJson(
    viewId: string,
    rowsJson: string,
    watermarkJson: string,
  ): Promise<void> {
    this.handle.setViewRowsJson(viewId, rowsJson, watermarkJson)
    return Promise.resolve()
  }

  closeView(viewId: string): Promise<void> {
    this.handle.closeView(viewId)
    return Promise.resolve()
  }

  ingestBatchJson(batchJson: string): Promise<void> {
    this.handle.ingestBatchJson(batchJson)
    return Promise.resolve()
  }

  acceptMutationJson(acceptJson: string): Promise<void> {
    this.handle.acceptMutationJson(acceptJson)
    return Promise.resolve()
  }

  settle(mutationId: string, outcome: SettlementVerdict): Promise<boolean> {
    return Promise.resolve(this.handle.settle(mutationId, outcome))
  }

  captureMutationDiffJson(
    messageId: string,
    assertionJson: string,
  ): Promise<string> {
    return Promise.resolve(
      this.handle.captureMutationDiffJson(messageId, assertionJson),
    )
  }

  mailboxJson(mailboxId: string): Promise<string> {
    return Promise.resolve(this.handle.mailboxJson(mailboxId))
  }

  projectViewJson(viewId: string): Promise<string> {
    return Promise.resolve(this.handle.projectViewJson(viewId))
  }

  drainDirtyJson(): Promise<string> {
    return Promise.resolve(this.handle.drainDirtyJson())
  }

  drainRetiredJson(): Promise<string> {
    return Promise.resolve(this.handle.drainRetiredJson())
  }
}
