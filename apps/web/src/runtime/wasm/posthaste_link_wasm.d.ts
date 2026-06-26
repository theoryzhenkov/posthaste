/* tslint:disable */
/* eslint-disable */

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
    readonly invertMessageChangeDiff: (a: number, b: number) => [number, number, number, number];
    readonly parseMessageMutation: (a: number, b: number) => [number, number, number, number];
    readonly __wbg_maillistreplicahandle_free: (a: number, b: number) => void;
    readonly maillistreplicahandle_acceptJson: (a: number, b: number, c: number) => [number, number];
    readonly maillistreplicahandle_hasPending: (a: number) => number;
    readonly maillistreplicahandle_ingestJson: (a: number, b: number, c: number) => [number, number];
    readonly maillistreplicahandle_new: () => number;
    readonly maillistreplicahandle_projectJson: (a: number, b: number, c: number) => [number, number, number, number];
    readonly maillistreplicahandle_settle: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
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
