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
  /**
   * The invertible change-diff a mutation would produce over a message's
   * current folded base, WITHOUT applying it: `from_before_after(prev, curr)`
   * where `prev` is the message's current fold state and `curr` is `prev` with
   * the assertion applied purely. Client-local diff capture for client-owned
   * undo history. `"null"` if the message is not held or the assertion destroys
   * it (non-invertible).
   */
  captureMutationDiffJson(messageId: string, assertionJson: string): string
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
  /** Drain the ids of ops retired since the last drain (JSON string array). The
   *  host clears durable-outbox records only for these. (outbox D) */
  drainRetiredJson(): string
}

export type EntityStoreHandleFactory = () => EntityStoreHandle

let cachedEntityStoreFactory: Promise<EntityStoreHandleFactory> | undefined

/**
 * Load + instantiate the WASM `EntityStore` module once, returning a factory
 * for fresh handles. Cached so the module initializes a single time. The
 * dynamic import keeps the WASM lazy — loaded once on startup when the
 * entityStoreAdapter installs (the sole read model).
 */
export function loadEntityStoreHandleFactory(): Promise<EntityStoreHandleFactory> {
  cachedEntityStoreFactory ??= (async () => {
    const module = await import('../wasm/posthaste_link_wasm.js')
    await module.default()
    return () => new module.EntityStoreHandle() as EntityStoreHandle
  })()
  return cachedEntityStoreFactory
}
