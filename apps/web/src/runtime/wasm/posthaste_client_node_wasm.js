/* @ts-self-types="./posthaste_client_node_wasm.d.ts" */

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
     * Capture the invertible change-diff a mutation would produce over a
     * message's current folded base, **without applying it**: reads the message's
     * optimistic fold state (`prev`), applies the assertion purely (`curr`),
     * and returns `MessageChangeDiff::from_before_after(prev, curr)` as JSON.
     * This is the client-local diff capture for client-owned undo history
     * ([undo-redo-synced-history] Phase 1 option a) — it mirrors the runtime's
     * `read_fold_state` + `capture_diff` over the store, so the two produce the
     * same diff for the same assertion + base.
     *
     * Returns `"null"` when the message is not held (no `prev` — the mutation
     * is deferred until the base arrives) or the assertion would destroy it
     * (non-invertible; `Destroy` is not diff-eligible). The host records no
     * history step in either case.
     *
     * `assertion_json` is the same `ReplicaAssertion` shape `acceptMutationJson`
     * takes (`{kind, ...}`), already role-resolved by `parseMailOperation`.
     * @param {string} message_id
     * @param {string} assertion_json
     * @returns {string}
     */
    captureMutationDiffJson(message_id, assertion_json) {
        let deferred4_0;
        let deferred4_1;
        try {
            const ptr0 = passStringToWasm0(message_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(assertion_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            const ret = wasm.entitystorehandle_captureMutationDiffJson(this.__wbg_ptr, ptr0, len0, ptr1, len1);
            var ptr3 = ret[0];
            var len3 = ret[1];
            if (ret[3]) {
                ptr3 = 0; len3 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred4_0 = ptr3;
            deferred4_1 = len3;
            return getStringFromWasm0(ptr3, len3);
        } finally {
            wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
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
     * The ids of ops retired since the last drain as a JSON array of strings
     * (or `"[]"`). The host clears durable-outbox records only for these — an
     * un-retired op is still pending in-engine and must survive a reload.
     * (outbox D)
     * @returns {string}
     */
    drainRetiredJson() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.entitystorehandle_drainRetiredJson(this.__wbg_ptr);
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
     * watermark?}` where `predicate` is `{"inMailboxes":[id,..]}` / `"all"` /
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
 * The near-end engine, driven from JS. Constructed with an IO object + a
 * config JSON string; `connect`/`disconnect`/`forward` return Promises.
 */
export class NearEndHandle {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        NearEndHandleFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_nearendhandle_free(ptr, 0);
    }
    /**
     * Open the link and start the frame loop (idempotent). The Promise
     * resolves once the link is open; the reconnect loop then runs in the
     * background (`spawn_local`) until [`Self::disconnect`].
     * @returns {Promise<any>}
     */
    connect() {
        const ret = wasm.nearendhandle_connect(this.__wbg_ptr);
        return ret;
    }
    /**
     * The engine-owned resume cursor (last seen `linkSeq`). The host mirrors
     * this to durable storage so a reload resumes where it left off — callers no
     * longer thread `afterSeq`.
     * @returns {number | undefined}
     */
    cursor() {
        const ret = wasm.nearendhandle_cursor(this.__wbg_ptr);
        return ret[0] === 0 ? undefined : ret[1];
    }
    /**
     * Stop the frame loop (no further reconnects). Link close is a host
     * concern (a policy-free `DELETE` via the api client) since the transport is
     * post-only.
     * @returns {Promise<any>}
     */
    disconnect() {
        const ret = wasm.nearendhandle_disconnect(this.__wbg_ptr);
        return ret;
    }
    /**
     * Forward a mutation (JSON of a `MutationRequest`). Resolves with the
     * receipt JSON on 2xx (including an authority `failed` verdict); rejects on
     * a permanent 4xx or exhausted transient retries.
     * @param {string} request_json
     * @returns {Promise<any>}
     */
    forward(request_json) {
        const ptr0 = passStringToWasm0(request_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.nearendhandle_forward(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    /**
     * The current link id, once connected.
     * @returns {string | undefined}
     */
    linkId() {
        const ret = wasm.nearendhandle_linkId(this.__wbg_ptr);
        let v1;
        if (ret[0] !== 0) {
            v1 = getStringFromWasm0(ret[0], ret[1]).slice();
            wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        }
        return v1;
    }
    /**
     * Build the engine from the JS IO object and a config JSON string.
     *
     * `io` must expose: `postJson(url, headersJson, body) => Promise<{status,
     * body}>`, `getJson(url, headersJson) => Promise<{status, body}>`,
     * `openStream(url, onEvent) => abortFn` (where `onEvent(kind, data,
     * status)`), `onFrame(json)`, `onMalformed(raw, error)`, `onReset()` (D49 —
     * re-seed the adapter), `onStatus(label, message)` (labels include
     * `degraded`), `onLinkReestablished(linkId)` (M44 recovery edge — a fresh
     * re-prepared link), `neverDispatched() => Promise<string>` (a JSON array of
     * forward requests), `onReconciled(receiptJson)`, `sentUnsettled() =>
     * Promise<string>` (a JSON array of `{linkId, clientMutationId,
     * request?}`), and `onSettlement(receiptJson)`.
     * @param {any} io
     * @param {string} config_json
     */
    constructor(io, config_json) {
        const ptr0 = passStringToWasm0(config_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.nearendhandle_new(io, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0] >>> 0;
        NearEndHandleFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
}
if (Symbol.dispose) NearEndHandle.prototype[Symbol.dispose] = NearEndHandle.prototype.free;

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
 * Parse a runtime mutation request (its flattened typed `MailOperation`) and
 * return `{ messageId, assertion }` as JSON when the operation is locally
 * foldable. Returns `null` for operations whose effect cannot be folded from
 * the request alone. `role_map_json` is the
 * account's role→mailbox-id map (`{"archive": "mbx-..."}`, built client-side
 * from the mailbox list); it resolves role moves (archive/trash/restoreToInbox/
 * moveToRole) to `ReplaceMailboxes`. `{}` → role moves get no optimism (graceful
 * when the mailbox list isn't loaded yet). Consumes the same
 * `MailOperation::fold_effect_with_roles` projection the Rust near node folds
 * with (D34 — one local-effect derivation, shared).
 * @param {string} request_json
 * @param {string} role_map_json
 * @returns {string | undefined}
 */
export function parseMailOperation(request_json, role_map_json) {
    const ptr0 = passStringToWasm0(request_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(role_map_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ret = wasm.parseMailOperation(ptr0, len0, ptr1, len1);
    if (ret[3]) {
        throw takeFromExternrefTable0(ret[2]);
    }
    let v3;
    if (ret[0] !== 0) {
        v3 = getStringFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v3;
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_2e59b1b37a9a34c3: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg___wbindgen_is_function_49868bde5eb1e745: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_undefined_c0cca72b82b86f4d: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_7579aab02a8a620c: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_914df97fcfa788f2: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_81fc77679af83bc6: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_3c3b4f651835fbcb: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_call_368fa9c372d473ba: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg0.call(arg1, arg2, arg3);
            return ret;
        }, arguments); },
        __wbg_call_7f2987183bb62793: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments); },
        __wbg_call_d578befcc3145dee: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_call_f2ac1622600b957f: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            const ret = arg0.call(arg1, arg2, arg3, arg4);
            return ret;
        }, arguments); },
        __wbg_clearTimeout_113b1cde814ec762: function(arg0) {
            const ret = clearTimeout(arg0);
            return ret;
        },
        __wbg_get_f96702c6245e4ef9: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_instanceof_Promise_95d523058012a13d: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Promise;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_new_typed_14d7cc391ce53d2c: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return wasm_bindgen__convert__closures_____invoke__h454f628c0b88f09d(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return ret;
            } finally {
                state0.a = 0;
            }
        },
        __wbg_queueMicrotask_abaf92f0bd4e80a4: function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        },
        __wbg_queueMicrotask_df5a6dac26d818f3: function(arg0) {
            queueMicrotask(arg0);
        },
        __wbg_random_a72d453e63c9558c: function() {
            const ret = Math.random();
            return ret;
        },
        __wbg_resolve_0a79de24e9d2267b: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_setTimeout_ef24d2fc3ad97385: function() { return handleError(function (arg0, arg1) {
            const ret = setTimeout(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_static_accessor_GLOBAL_THIS_a1248013d790bf5f: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_f2e0f995a21329ff: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_24f78b6d23f286ea: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_59fd959c540fe405: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_then_00eed3ac0b8e82cb: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_then_a0c8db0381c8994c: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 240, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__ha35c10aed9720f95);
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [String, String, F64], shim_idx: 140, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h28e6bf82b92400ea);
            return ret;
        },
        __wbindgen_cast_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [], shim_idx: 159, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h690adb9d64021208);
            return ret;
        },
        __wbindgen_cast_0000000000000004: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
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
        "./posthaste_client_node_wasm_bg.js": import0,
    };
}

function wasm_bindgen__convert__closures_____invoke__h690adb9d64021208(arg0, arg1) {
    wasm.wasm_bindgen__convert__closures_____invoke__h690adb9d64021208(arg0, arg1);
}

function wasm_bindgen__convert__closures_____invoke__ha35c10aed9720f95(arg0, arg1, arg2) {
    const ret = wasm.wasm_bindgen__convert__closures_____invoke__ha35c10aed9720f95(arg0, arg1, arg2);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}

function wasm_bindgen__convert__closures_____invoke__h454f628c0b88f09d(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen__convert__closures_____invoke__h454f628c0b88f09d(arg0, arg1, arg2, arg3);
}

function wasm_bindgen__convert__closures_____invoke__h28e6bf82b92400ea(arg0, arg1, arg2, arg3, arg4) {
    const ptr0 = passStringToWasm0(arg2, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(arg3, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    wasm.wasm_bindgen__convert__closures_____invoke__h28e6bf82b92400ea(arg0, arg1, ptr0, len0, ptr1, len1, arg4);
}

const EntityStoreHandleFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_entitystorehandle_free(ptr >>> 0, 1));
const NearEndHandleFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_nearendhandle_free(ptr >>> 0, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => wasm.__wbindgen_destroy_closure(state.a, state.b));

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

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

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
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
    cachedDataViewMemory0 = null;
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
        module_or_path = new URL('posthaste_client_node_wasm_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
