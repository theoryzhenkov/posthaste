/* @ts-self-types="./posthaste_link_wasm.d.ts" */

/**
 * A live reactive entity store owned by JS: messages, mailboxes (count
 * scalars), and views (ordered row lists + coverage), with a message optimism
 * fold. The host feeds it authoritative batches and reads the dirty keys to
 * drive the renderer.
 */
export class EntityStoreHandle {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        EntityStoreHandleFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_entitystorehandle_free(ptr, 0);
    }
    /**
     * Accept an optimistic message mutation into the outbox (idempotent on
     * mutation id). `accept_json` is `{mutationId, messageId, assertion}`.
     * The projected state is re-derived so reads reflect the fold
     * immediately; a mutation on a not-yet-ingested message is tracked but
     * deferred.
     * @param {string} accept_json
     */
    acceptMutationJson(accept_json) {
        const ptr0 = passStringToWasm0(accept_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.entitystorehandle_acceptMutationJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Close a view (it was closed on the host).
     * @param {string} view_id
     */
    closeView(view_id) {
        const ptr0 = passStringToWasm0(view_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.entitystorehandle_closeView(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Drain the keys changed since the last drain as a JSON array of
     * `{"message":id}` / `{"mailbox":id}` / `{"view":id}`. The host re-reads
     * these (re-project views, re-write counts). One drain per batch.
     * @returns {string}
     */
    drainDirtyJson() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.entitystorehandle_drainDirtyJson(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Whether any optimistic mutation is still pending.
     * @returns {boolean}
     */
    hasPending() {
        const ret = wasm.entitystorehandle_hasPending(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Apply an authoritative batch atomically: every update is applied before
     * any dirty key is reported. `batch_json` is a JSON array of
     * `{"message":{messageId, projection, deleted, countDeltas:[{mailboxId,
     * unreadCount, totalCount}]}}` and/or `{"mailboxCount":{mailboxId,
     * unreadCount, totalCount}}`.
     * @param {string} batch_json
     */
    ingestBatchJson(batch_json) {
        const ptr0 = passStringToWasm0(batch_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.entitystorehandle_ingestBatchJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * A mailbox's server-authoritative counts as `{"unreadCount",
     * "totalCount"}`, or `"null"` if the mailbox is not held.
     * @param {string} mailbox_id
     * @returns {string}
     */
    mailboxJson(mailbox_id) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ptr0 = passStringToWasm0(mailbox_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.entitystorehandle_mailboxJson(this.__wbg_ptr, ptr0, len0);
            deferred2_0 = ret[0];
            deferred2_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * A message's optimistic projection as a JSON string, or `"null"` if the
     * message is not held or has been optimistically destroyed. The projection
     * is the confirmed base with the pending outbox folded over it (keywords +
     * mailbox membership) — never stored as truth.
     * @param {string} message_id
     * @returns {string}
     */
    messageJson(message_id) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ptr0 = passStringToWasm0(message_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.entitystorehandle_messageJson(this.__wbg_ptr, ptr0, len0);
            deferred2_0 = ret[0];
            deferred2_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    constructor() {
        const ret = wasm.entitystorehandle_new();
        this.__wbg_ptr = ret >>> 0;
        EntityStoreHandleFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * A view's **projected** rows as a JSON array of `{rowKey, messageId,
     * sortKey, projection}` — the optimistic message projection joined to each
     * row in one call, so the host re-projects a view with a single round-trip
     * per drain (P1: a row implies a live base, so `projection` is non-null for
     * every placed row). `"null"` if the view is not registered.
     * @param {string} view_id
     * @returns {string}
     */
    projectViewJson(view_id) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ptr0 = passStringToWasm0(view_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.entitystorehandle_projectViewJson(this.__wbg_ptr, ptr0, len0);
            deferred2_0 = ret[0];
            deferred2_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Register a view with its predicate, sort, and initial coverage
     * watermark. The host calls this when a view is opened (or its window
     * grows). `args_json` is `{predicate, sortField, sortDirection,
     * watermark?}` where `predicate` is `{"inMailbox":id}` / `"all"` /
     * `"deferred"` and `watermark` is `{"receivedAt","messageId"}` or null
     * (reaches BOTTOM). Marks the view dirty.
     * @param {string} view_id
     * @param {string} args_json
     */
    registerViewJson(view_id, args_json) {
        const ptr0 = passStringToWasm0(view_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(args_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.entitystorehandle_registerViewJson(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Replace a view's held rows + watermark (a served snapshot / page /
     * resync). `rows_json` is a JSON array of `{rowKey, messageId,
     * sortKey:{receivedAt,messageId}}`; `watermark_json` is the new watermark
     * (`{"receivedAt","messageId"}` or `null`). Does not touch the message
     * base — the host ingests the rows' projections atomically in the same
     * batch via [`ingest_batch_json`](Self::ingest_batch_json) (P1: a row
     * implies a live base).
     * @param {string} view_id
     * @param {string} rows_json
     * @param {string} watermark_json
     */
    setViewRowsJson(view_id, rows_json, watermark_json) {
        const ptr0 = passStringToWasm0(view_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(rows_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(watermark_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len2 = WASM_VECTOR_LEN;
        const ret = wasm.entitystorehandle_setViewRowsJson(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Settle a pending mutation by its terminal outcome. `outcome` is
     * `"confirmed"` or `"failed"`. Returns `true` when the settlement reverted
     * an optimistic change (a failure the host should surface).
     * @param {string} mutation_id
     * @param {string} outcome
     * @returns {boolean}
     */
    settle(mutation_id, outcome) {
        const ptr0 = passStringToWasm0(mutation_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(outcome, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.entitystorehandle_settle(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
    /**
     * A view's rows as a JSON array of `{rowKey, messageId, sortKey}`, or
     * `"null"` if the view is not registered.
     * @param {string} view_id
     * @returns {string}
     */
    viewRowsJson(view_id) {
        let deferred2_0;
        let deferred2_1;
        try {
            const ptr0 = passStringToWasm0(view_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.entitystorehandle_viewRowsJson(this.__wbg_ptr, ptr0, len0);
            deferred2_0 = ret[0];
            deferred2_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
    }
}
if (Symbol.dispose) EntityStoreHandle.prototype[Symbol.dispose] = EntityStoreHandle.prototype.free;

/**
 * A live mail-list replica owned by JS: the served rows are its base, the
 * outbox holds optimistic mutations, and `projectJson` returns the folded rows.
 */
export class MailListReplicaHandle {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        MailListReplicaHandleFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_maillistreplicahandle_free(ptr, 0);
    }
    /**
     * Accept an optimistic mutation: `accept_json` is `{mutationId, messageId,
     * assertion}` where `assertion` is `{kind:"setKeywords",add,remove}` /
     * `{kind:"replaceMailboxes",mailboxIds}` / `{kind:"destroy"}`.
     * @param {string} accept_json
     */
    acceptJson(accept_json) {
        const ptr0 = passStringToWasm0(accept_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.maillistreplicahandle_acceptJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {boolean}
     */
    hasPending() {
        const ret = wasm.maillistreplicahandle_hasPending(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Adopt a served base: `rows_json` is a JSON array of `{messageId,
     * projection}` (the host maps it from the runtime's `MailListViewState`).
     * @param {string} rows_json
     */
    ingestJson(rows_json) {
        const ptr0 = passStringToWasm0(rows_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.maillistreplicahandle_ingestJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    constructor() {
        const ret = wasm.maillistreplicahandle_new();
        this.__wbg_ptr = ret >>> 0;
        MailListReplicaHandleFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * The optimistic rows as a JSON array of projections, in served order. When
     * `mailbox_id` is provided, rows whose folded membership no longer includes
     * it are dropped (concrete-mailbox archive-out); otherwise only destroyed
     * rows drop and membership is left to the runtime's next served base.
     * @param {string | null} [mailbox_id]
     * @returns {string}
     */
    projectJson(mailbox_id) {
        let deferred3_0;
        let deferred3_1;
        try {
            var ptr0 = isLikeNone(mailbox_id) ? 0 : passStringToWasm0(mailbox_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len0 = WASM_VECTOR_LEN;
            const ret = wasm.maillistreplicahandle_projectJson(this.__wbg_ptr, ptr0, len0);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * Settle a pending mutation. `outcome` is `"confirmed"` or `"failed"`.
     * Returns `true` when the settlement reverted an optimistic change (a
     * failure the host should surface).
     * @param {string} mutation_id
     * @param {string} outcome
     * @returns {boolean}
     */
    settle(mutation_id, outcome) {
        const ptr0 = passStringToWasm0(mutation_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(outcome, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.maillistreplicahandle_settle(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
}
if (Symbol.dispose) MailListReplicaHandle.prototype[Symbol.dispose] = MailListReplicaHandle.prototype.free;

/**
 * A mail-list replica that operates on full runtime view-state rows.
 *
 * The host owns transport and persistence; this boundary owns the fold.
 */
export class RuntimeMailListReplica {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        RuntimeMailListReplicaFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_runtimemaillistreplica_free(ptr, 0);
    }
    /**
     * Accept an optimistic mutation by assertion JSON, the same shape used by
     * `MailListReplicaHandle`.
     * @param {string} accept_json
     */
    acceptJson(accept_json) {
        const ptr0 = passStringToWasm0(accept_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.runtimemaillistreplica_acceptJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Apply a runtime `MailListDelta` to the confirmed base.
     *
     * When `order` is present, rows whose `row_key` is absent are dropped and
     * the rest are reordered; `upserts` replace rows by `row_key`. Pending
     * mutations are preserved and re-folded.
     * @param {string} delta_json
     */
    applyDeltaJson(delta_json) {
        const ptr0 = passStringToWasm0(delta_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.runtimemaillistreplica_applyDeltaJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * @returns {boolean}
     */
    hasPending() {
        const ret = wasm.runtimemaillistreplica_hasPending(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Adopt a served `MailListViewState` rows array as the confirmed base.
     * Replaces the base and drops any rows no longer present, but keeps
     * pending mutations so they re-fold over the new base.
     * @param {string} rows_json
     */
    ingestViewJson(rows_json) {
        const ptr0 = passStringToWasm0(rows_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.runtimemaillistreplica_ingestViewJson(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    constructor() {
        const ret = wasm.runtimemaillistreplica_new();
        this.__wbg_ptr = ret >>> 0;
        RuntimeMailListReplicaFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Return the optimistic rows as a JSON array of full `MailListRowState`.
     * Pass the viewed concrete `mailbox_id` to drop archive-out rows.
     * @param {string | null} [mailbox_id]
     * @returns {string}
     */
    projectViewJson(mailbox_id) {
        let deferred3_0;
        let deferred3_1;
        try {
            var ptr0 = isLikeNone(mailbox_id) ? 0 : passStringToWasm0(mailbox_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len0 = WASM_VECTOR_LEN;
            const ret = wasm.runtimemaillistreplica_projectViewJson(this.__wbg_ptr, ptr0, len0);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * Settle a pending mutation. Returns `true` when the settlement reverted
     * an optimistic change.
     * @param {string} mutation_id
     * @param {string} outcome
     * @returns {boolean}
     */
    settle(mutation_id, outcome) {
        const ptr0 = passStringToWasm0(mutation_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(outcome, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.runtimemaillistreplica_settle(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] !== 0;
    }
}
if (Symbol.dispose) RuntimeMailListReplica.prototype[Symbol.dispose] = RuntimeMailListReplica.prototype.free;

/**
 * Swap added↔removed for both the keyword and mailbox facets — the inverse
 * diff applied by undo. Uses `MessageChangeDiff::inverse` in Rust.
 * @param {string} diff_json
 * @returns {string}
 */
export function invertMessageChangeDiff(diff_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(diff_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.invertMessageChangeDiff(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Parse a runtime mutation request and return `{ messageId, assertion }` as
 * JSON when the mutation is locally foldable. Returns `null` for mutations
 * whose effect cannot be folded from the request alone (role moves such as
 * archive/trash/moveToRole). Mirrors the Rust near-node
 * `MessageMutation::from_request` + `to_assertion` path.
 * @param {string} request_json
 * @returns {string | undefined}
 */
export function parseMessageMutation(request_json) {
    const ptr0 = passStringToWasm0(request_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.parseMessageMutation(ptr0, len0);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    let v2;
    if (ret[0] !== 0) {
        v2 = getStringFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v2;
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_2e59b1b37a9a34c3: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_throw_81fc77679af83bc6: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./posthaste_link_wasm_bg.js": import0,
    };
}

const EntityStoreHandleFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_entitystorehandle_free(ptr >>> 0, 1));
const MailListReplicaHandleFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_maillistreplicahandle_free(ptr >>> 0, 1));
const RuntimeMailListReplicaFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_runtimemaillistreplica_free(ptr >>> 0, 1));

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasm;
function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    wasmModule = module;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('posthaste_link_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
