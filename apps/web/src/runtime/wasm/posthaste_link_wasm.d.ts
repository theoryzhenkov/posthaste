/* tslint:disable */
/* eslint-disable */

/**
 * A live reactive entity store owned by JS: messages, mailboxes (count
 * scalars), and views (ordered row lists + coverage), with a message optimism
 * fold. The host feeds it authoritative batches and reads the dirty keys to
 * drive the renderer.
 */
export class EntityStoreHandle {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Accept an optimistic message mutation into the outbox (idempotent on
     * mutation id). `accept_json` is `{mutationId, messageId, assertion}`.
     * The projected state is re-derived so reads reflect the fold
     * immediately; a mutation on a not-yet-ingested message is tracked but
     * deferred.
     */
    acceptMutationJson(accept_json: string): void;
    /**
     * Close a view (it was closed on the host).
     */
    closeView(view_id: string): void;
    /**
     * Drain the keys changed since the last drain as a JSON array of
     * `{"message":id}` / `{"mailbox":id}` / `{"view":id}`. The host re-reads
     * these (re-project views, re-write counts). One drain per batch.
     */
    drainDirtyJson(): string;
    /**
     * Whether any optimistic mutation is still pending.
     */
    hasPending(): boolean;
    /**
     * Apply an authoritative batch atomically: every update is applied before
     * any dirty key is reported. `batch_json` is a JSON array of
     * `{"message":{messageId, projection, deleted, countDeltas:[{mailboxId,
     * unreadCount, totalCount}]}}` and/or `{"mailboxCount":{mailboxId,
     * unreadCount, totalCount}}`.
     */
    ingestBatchJson(batch_json: string): void;
    /**
     * A mailbox's server-authoritative counts as `{"unreadCount",
     * "totalCount"}`, or `"null"` if the mailbox is not held.
     */
    mailboxJson(mailbox_id: string): string;
    /**
     * A message's optimistic projection as a JSON string, or `"null"` if the
     * message is not held or has been optimistically destroyed. The projection
     * is the confirmed base with the pending outbox folded over it (keywords +
     * mailbox membership) — never stored as truth.
     */
    messageJson(message_id: string): string;
    constructor();
    /**
     * A view's **projected** rows as a JSON array of `{rowKey, messageId,
     * sortKey, projection}` — the optimistic message projection joined to each
     * row in one call, so the host re-projects a view with a single round-trip
     * per drain (P1: a row implies a live base, so `projection` is non-null for
     * every placed row). `"null"` if the view is not registered.
     */
    projectViewJson(view_id: string): string;
    /**
     * Register a view with its predicate, sort, and initial coverage
     * watermark. The host calls this when a view is opened (or its window
     * grows). `args_json` is `{predicate, sortField, sortDirection,
     * watermark?}` where `predicate` is `{"inMailbox":id}` / `"all"` /
     * `"deferred"` and `watermark` is `{"receivedAt","messageId"}` or null
     * (reaches BOTTOM). Marks the view dirty.
     */
    registerViewJson(view_id: string, args_json: string): void;
    /**
     * Replace a view's held rows + watermark (a served snapshot / page /
     * resync). `rows_json` is a JSON array of `{rowKey, messageId,
     * sortKey:{receivedAt,messageId}}`; `watermark_json` is the new watermark
     * (`{"receivedAt","messageId"}` or `null`). Does not touch the message
     * base — the host ingests the rows' projections atomically in the same
     * batch via [`ingest_batch_json`](Self::ingest_batch_json) (P1: a row
     * implies a live base).
     */
    setViewRowsJson(view_id: string, rows_json: string, watermark_json: string): void;
    /**
     * Settle a pending mutation by its terminal outcome. `outcome` is
     * `"confirmed"` or `"failed"`. Returns `true` when the settlement reverted
     * an optimistic change (a failure the host should surface).
     */
    settle(mutation_id: string, outcome: string): boolean;
    /**
     * A view's rows as a JSON array of `{rowKey, messageId, sortKey}`, or
     * `"null"` if the view is not registered.
     */
    viewRowsJson(view_id: string): string;
}

/**
 * A live mail-list replica owned by JS: the served rows are its base, the
 * outbox holds optimistic mutations, and `projectJson` returns the folded rows.
 */
export class MailListReplicaHandle {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Accept an optimistic mutation: `accept_json` is `{mutationId, messageId,
     * assertion}` where `assertion` is `{kind:"setKeywords",add,remove}` /
     * `{kind:"replaceMailboxes",mailboxIds}` / `{kind:"destroy"}`.
     */
    acceptJson(accept_json: string): void;
    hasPending(): boolean;
    /**
     * Adopt a served base: `rows_json` is a JSON array of `{messageId,
     * projection}` (the host maps it from the runtime's `MailListViewState`).
     */
    ingestJson(rows_json: string): void;
    constructor();
    /**
     * The optimistic rows as a JSON array of projections, in served order. When
     * `mailbox_id` is provided, rows whose folded membership no longer includes
     * it are dropped (concrete-mailbox archive-out); otherwise only destroyed
     * rows drop and membership is left to the runtime's next served base.
     */
    projectJson(mailbox_id?: string | null): string;
    /**
     * Settle a pending mutation. `outcome` is `"confirmed"` or `"failed"`.
     * Returns `true` when the settlement reverted an optimistic change (a
     * failure the host should surface).
     */
    settle(mutation_id: string, outcome: string): boolean;
}

/**
 * A mail-list replica that operates on full runtime view-state rows.
 *
 * The host owns transport and persistence; this boundary owns the fold.
 */
export class RuntimeMailListReplica {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Accept an optimistic mutation by assertion JSON, the same shape used by
     * `MailListReplicaHandle`.
     */
    acceptJson(accept_json: string): void;
    /**
     * Apply a runtime `MailListDelta` to the confirmed base.
     *
     * When `order` is present, rows whose `row_key` is absent are dropped and
     * the rest are reordered; `upserts` replace rows by `row_key`. Pending
     * mutations are preserved and re-folded.
     */
    applyDeltaJson(delta_json: string): void;
    hasPending(): boolean;
    /**
     * Adopt a served `MailListViewState` rows array as the confirmed base.
     * Replaces the base and drops any rows no longer present, but keeps
     * pending mutations so they re-fold over the new base.
     */
    ingestViewJson(rows_json: string): void;
    constructor();
    /**
     * Return the optimistic rows as a JSON array of full `MailListRowState`.
     * Pass the viewed concrete `mailbox_id` to drop archive-out rows.
     */
    projectViewJson(mailbox_id?: string | null): string;
    /**
     * Settle a pending mutation. Returns `true` when the settlement reverted
     * an optimistic change.
     */
    settle(mutation_id: string, outcome: string): boolean;
}

/**
 * Swap added↔removed for both the keyword and mailbox facets — the inverse
 * diff applied by undo. Uses `MessageChangeDiff::inverse` in Rust.
 */
export function invertMessageChangeDiff(diff_json: string): string;

/**
 * Parse a runtime mutation request and return `{ messageId, assertion }` as
 * JSON when the mutation is locally foldable. Returns `null` for mutations
 * whose effect cannot be folded from the request alone (role moves such as
 * archive/trash/moveToRole). Mirrors the Rust near-node
 * `MessageMutation::from_request` + `to_assertion` path.
 */
export function parseMessageMutation(request_json: string): string | undefined;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_runtimemaillistreplica_free: (a: number, b: number) => void;
    readonly runtimemaillistreplica_acceptJson: (a: number, b: number, c: number) => [number, number];
    readonly runtimemaillistreplica_applyDeltaJson: (a: number, b: number, c: number) => [number, number];
    readonly runtimemaillistreplica_hasPending: (a: number) => number;
    readonly runtimemaillistreplica_ingestViewJson: (a: number, b: number, c: number) => [number, number];
    readonly runtimemaillistreplica_new: () => number;
    readonly runtimemaillistreplica_projectViewJson: (a: number, b: number, c: number) => [number, number, number, number];
    readonly runtimemaillistreplica_settle: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly __wbg_entitystorehandle_free: (a: number, b: number) => void;
    readonly entitystorehandle_acceptMutationJson: (a: number, b: number, c: number) => [number, number];
    readonly entitystorehandle_closeView: (a: number, b: number, c: number) => void;
    readonly entitystorehandle_drainDirtyJson: (a: number) => [number, number];
    readonly entitystorehandle_hasPending: (a: number) => number;
    readonly entitystorehandle_ingestBatchJson: (a: number, b: number, c: number) => [number, number];
    readonly entitystorehandle_mailboxJson: (a: number, b: number, c: number) => [number, number];
    readonly entitystorehandle_messageJson: (a: number, b: number, c: number) => [number, number];
    readonly entitystorehandle_new: () => number;
    readonly entitystorehandle_projectViewJson: (a: number, b: number, c: number) => [number, number];
    readonly entitystorehandle_registerViewJson: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly entitystorehandle_setViewRowsJson: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly entitystorehandle_settle: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly entitystorehandle_viewRowsJson: (a: number, b: number, c: number) => [number, number];
    readonly __wbg_maillistreplicahandle_free: (a: number, b: number) => void;
    readonly maillistreplicahandle_acceptJson: (a: number, b: number, c: number) => [number, number];
    readonly maillistreplicahandle_hasPending: (a: number) => number;
    readonly maillistreplicahandle_ingestJson: (a: number, b: number, c: number) => [number, number];
    readonly maillistreplicahandle_new: () => number;
    readonly maillistreplicahandle_projectJson: (a: number, b: number, c: number) => [number, number, number, number];
    readonly maillistreplicahandle_settle: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly invertMessageChangeDiff: (a: number, b: number) => [number, number, number, number];
    readonly parseMessageMutation: (a: number, b: number) => [number, number, number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
