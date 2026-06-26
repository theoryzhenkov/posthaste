/**
 * The narrow JS surface of the WASM replica boundary
 * (`MailListReplicaHandle`), declared independently of the generated module so
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

export interface ReplicaHandle {
  /** Adopt a served base: a JSON array of `{messageId, projection}`. */
  ingestJson(rowsJson: string): void
  /** Accept an optimistic mutation: `{mutationId, messageId, assertion}`. */
  acceptJson(acceptJson: string): void
  /**
   * Retire a pending mutation. Returns `true` when a failure reverted optimism
   * (the host should surface it).
   */
  settle(mutationId: string, outcome: SettlementVerdict): boolean
  hasPending(): boolean
  /**
   * The optimistic rows as a JSON array of projections, in served order. Pass
   * the view's concrete mailbox to drop archived-out rows; omit it to defer
   * membership to the runtime's next served base.
   */
  projectJson(mailboxId?: string | null): string
}

export type ReplicaHandleFactory = () => ReplicaHandle

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
    return () => new module.MailListReplicaHandle() as ReplicaHandle
  })()
  return cachedFactory
}
