/**
 * The narrow JS surface of the WASM replica boundary
 * (`RuntimeMailListReplica`), declared independently of the generated module so
 * the host glue and its tests depend on this interface rather than on the
 * artifact. Production resolves it from the generated bundle via
 * {@link loadReplicaHandleFactory}; tests substitute an in-memory fake.
 *
 * @spec docs/replication/client-link/L2#3-the-wasm-boundary-posthaste-link-wasm
 */
export type SettlementVerdict = 'confirmed' | 'failed'

/**
 * An optimistic message assertion, mirroring the replica predictor's vocabulary.
 * These are folded over the served base; they carry no message body, recipient,
 * or auth material — only keyword/mailbox intent plus destroy.
 */
export type ReplicaAssertion =
  | { kind: 'setKeywords'; add: string[]; remove: string[] }
  | { kind: 'replaceMailboxes'; mailboxIds: string[] }
  | { kind: 'destroy' }
  | { kind: 'applyDiff'; diff: MessageChangeDiff }

/** An add/remove delta over one facet of a message's mutable state. */
export interface KeywordDelta {
  added: string[]
  removed: string[]
}

/**
 * An invertible change-diff over a message's mutable state: keywords + mailbox
 * membership, each an add/remove pair. Mirrors `posthaste-link-core`'s
 * `MessageChangeDiff`; the canonical inverse is computed in WASM via
 * {@link ../wasmUtil#invertMessageChangeDiff}.
 */
export interface MessageChangeDiff {
  keywords: KeywordDelta
  mailboxes: KeywordDelta
}

/**
 * One recorded reversible step on the session's undo/redo history, broadcast via
 * the `mutationHistory` frame's `undoTop`/`redoTop`. The client constructs the
 * undo/redo `message.applyDiff` mutation from it (undo applies the inverse diff,
 * redo applies the diff), carrying `seq` as the `undoOf`/`redoOf` hint.
 */
export interface DiffStep {
  seq: number
  messageId: string
  sourceId: string
  diff: MessageChangeDiff
}

export interface RuntimeReplicaHandle {
  /** Adopt a served `MailListViewState` rows array as the confirmed base. */
  ingestViewJson(rowsJson: string): void
  /** Apply a runtime `MailListDelta` to the confirmed base. */
  applyDeltaJson(deltaJson: string): void
  /** Accept an optimistic mutation: `{mutationId, messageId, assertion}`. */
  acceptJson(acceptJson: string): void
  /**
   * Retire a pending mutation. Returns `true` when a failure reverted optimism
   * (the host should surface it).
   */
  settle(mutationId: string, outcome: SettlementVerdict): boolean
  hasPending(): boolean
  /**
   * The optimistic rows as a JSON array of full `MailListRowState`. Pass the
   * view's concrete mailbox to drop archived-out rows; omit it to defer
   * membership to the runtime's next served base.
   */
  projectViewJson(mailboxId?: string | null): string
}

export type ReplicaHandleFactory = () => RuntimeReplicaHandle

let cachedFactory: Promise<ReplicaHandleFactory> | undefined

/**
 * Load and instantiate the generated WASM module once, returning a factory for
 * fresh per-view handles. Cached so the module initializes a single time.
 *
 * The dynamic import keeps the WASM out of the main bundle unless the
 * replicaAdapter is actually selected (VITE_RUNTIME_REPLICA).
 */
export function loadReplicaHandleFactory(): Promise<ReplicaHandleFactory> {
  cachedFactory ??= (async () => {
    const module = await import('../wasm/posthaste_link_wasm.js')
    await module.default()
    return () => new module.RuntimeMailListReplica() as RuntimeReplicaHandle
  })()
  return cachedFactory
}

/**
 * The narrow JS surface of the WASM `EntityStoreHandle` (slice 2e), declared
 * independently of the generated module so the host glue + its tests depend on
 * this interface. Production resolves it from the generated bundle via
 * {@link loadEntityStoreHandleFactory}; tests substitute an in-memory fake.
 *
 * Values cross the boundary as JSON strings (camelCase, externally-tagged) —
 * the wire contract is pinned by `entity_store::tests` in `posthaste-link-replica`
 * + the end-to-end handle tests in `posthaste-link-wasm`.
 *
 * @spec docs/eph/DESIGN-L2-client-link-reactive-store (2e)
 */
export interface EntityStoreHandle {
  /** Register a view: `{predicate, sortField, sortDirection, watermark?}`. */
  registerViewJson(viewId: string, argsJson: string): void
  /** Replace a view's rows + watermark (a served snapshot / page / resync). */
  setViewRowsJson(viewId: string, rowsJson: string, watermarkJson: string): void
  closeView(viewId: string): void
  /** Apply an authoritative batch atomically. */
  ingestBatchJson(batchJson: string): void
  /** Accept an optimistic mutation: `{mutationId, messageId, assertion}`. */
  acceptMutationJson(acceptJson: string): void
  /** Settle a pending mutation; returns `true` when a failure reverted. */
  settle(mutationId: string, outcome: SettlementVerdict): boolean
  hasPending(): boolean
  /** A message's optimistic projection JSON, or `"null"`. */
  messageJson(messageId: string): string
  /** A mailbox's counts `{unreadCount, totalCount}` JSON, or `"null"`. */
  mailboxJson(mailboxId: string): string
  /** A view's rows JSON (`[{rowKey, messageId, sortKey}]`), or `"null"`. */
  viewRowsJson(viewId: string): string
  /**
   * A view's projected rows (`[{rowKey, messageId, sortKey, projection}]`) —
   * the optimistic projection joined to each row in one call. `"null"` if the
   * view is not registered.
   */
  projectViewJson(viewId: string): string
  /** Drain the dirty keys (`[{message|mailbox|view: id}]`) since the last drain. */
  drainDirtyJson(): string
}

export type EntityStoreHandleFactory = () => EntityStoreHandle

let cachedEntityStoreFactory: Promise<EntityStoreHandleFactory> | undefined

/**
 * Load + instantiate the WASM `EntityStore` module once, returning a factory
 * for fresh handles. Cached so the module initializes a single time. The
 * dynamic import keeps the WASM out of the main bundle unless the
 * entityStoreAdapter is actually selected (`VITE_ENTITY_STORE`).
 */
export function loadEntityStoreHandleFactory(): Promise<EntityStoreHandleFactory> {
  cachedEntityStoreFactory ??= (async () => {
    const module = await import('../wasm/posthaste_link_wasm.js')
    await module.default()
    return () => new module.EntityStoreHandle() as EntityStoreHandle
  })()
  return cachedEntityStoreFactory
}
