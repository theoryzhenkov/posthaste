//! The runtime-side, **apply-scoped** idempotency ledger (RFC-L2-scripting D53 —
//! resolves architecture-cleanup P8: the REST direct-apply path had no
//! idempotency ledger).
//!
//! A script consuming the tap is an at-least-once consumer, so its write-back
//! must dedupe: a redelivered event that re-runs `apply` under the same
//! client-supplied key must return the first outcome, never re-execute. This
//! reuses the far-end up-half [`DedupStore`] — the same ledger the authority
//! server uses to dedup runtimes' forwarded mutations — as a small instance
//! dedicated to the direct-apply path, keyed by the **caller-scoped key**
//! `(ApplyScope, ClientMutationId)` (mirroring the replica path's
//! `(AuthorityServerLinkId, ClientMutationId)`).
//!
//! D47/D48 semantics are inherited from [`DedupStore`]:
//! - a `Confirmed` outcome is kept and re-observed;
//! - a `Rejected` (permanent) outcome is kept and re-observed — never re-executed;
//! - a transient `Failed` outcome clears the record so a deliberate retry
//!   re-executes.
//!
//! Reusing a key with a *different* operation is rejected (`Conflict`) — the
//! replica path's rule. There is no settlement frame on this path
//! (`settlement_seq = None`), so acked-cursor eviction never fires; the
//! in-memory ledger is bounded by the TTL and the safety-valve cap, reaped
//! opportunistically on each reservation (no background reaper runs on the
//! direct-apply path).
//!
//! **Durability (DS7 — mail-safety durability).** The in-memory [`DedupStore`]
//! alone is a TTL/cap-bounded cache lost on restart, so a redelivery arriving
//! after the TTL reap or after a process restart would re-execute an
//! already-applied keyed operation — a possible double-send. When a
//! [`DurableApplyStore`] is wired (the co-located build wires the authority
//! server's SQLite `apply_ledger` table; a store-less remote near node has
//! none), the DURABLE record is the source of truth for "already applied":
//!
//! - `reserve` writes a durable `pending` marker BEFORE returning `Execute`
//!   (atomically with the durable duplicate lookup), so an operation can never
//!   apply without a durable trace of the reservation;
//! - `settle` records the terminal decision durably (Confirmed outcome /
//!   Rejected envelope kept; a transient failure clears the row so a
//!   deliberate retry re-executes) before mirroring it into the in-memory
//!   cache;
//! - a redelivery whose key misses the in-memory cache (TTL-reaped, or a fresh
//!   process) consults the durable store and re-observes the recorded
//!   decision — never re-executing. An unresolved durable `pending` (a crash
//!   between apply and record) is conservatively surfaced as a permanent
//!   `Conflict` ("outcome unknown") rather than re-executed: for exactly-once
//!   send, not re-sending dominates; the caller must use a NEW key if it
//!   really intends a new operation.
//!
//! Retention is the durable store's concern: settled decisions are kept for a
//! horizon that dominates any realistic redelivery window (30 days in the
//! SQLite store, vs. the in-memory 15-minute TTL); `pending` crash markers are
//! never reaped. The in-memory map remains as a fast cache and the in-flight
//! (concurrent-duplicate) guard; its TTL/cap eviction is now safe because
//! every terminal decision it evicts is still durably recorded.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use posthaste_contract_core::{
    ClientMutationId, RuntimeAdapterError, RuntimeCaller, RuntimeError, RuntimeErrorCode,
    Terminality,
};
use posthaste_domain_model::{CommandAck, Operation};
use posthaste_link_far_end::up::{Accept, DedupStore, TerminalClass};
use serde::{Deserialize, Serialize};

/// Canonical op-name for a keyed draft save, distinct from every `MailOperation`
/// name and from [`DRAFT_DELETE_OP`] so a key reused across draft save/delete (or
/// any command) Conflicts on the ledger's op-name guard (D128).
pub(crate) const DRAFT_SAVE_OP: &str = "draft.save";
/// Canonical op-name for a keyed draft delete (see [`DRAFT_SAVE_OP`]).
pub(crate) const DRAFT_DELETE_OP: &str = "draft.delete";

/// The payload a settled reservation carries for replay. The direct-apply
/// command routes settle a [`CommandAck`]; the draft routes settle the enqueued
/// [`Operation`] so a replayed save/delete returns the SAME operation id and
/// response, not a fresh one (RFC-L2-drafts D128 — "a real unit, not a
/// wrapper"). One ledger serves both families: a same-key-different-op reuse
/// Conflicts on the op-name guard *before* any payload is returned, so the
/// variant a replay yields always matches the op that stored it.
#[derive(Clone)]
pub(crate) enum AppliedOutcome {
    /// A direct-apply command ack (set-keywords, mailbox add/remove/replace,
    /// destroy). Also what a keyed `message.send` settled BEFORE sends stored
    /// their operation — the send route tolerates such a legacy record on
    /// replay (the send applied; only its operation payload is unavailable).
    Ack(CommandAck),
    /// An enqueued operation: `draft.save` / `draft.delete` — and, since
    /// scheduled sends, `message.send` (a replayed keyed send re-observes the
    /// SAME operation, so a scheduled send's cancel handle is stable across
    /// redeliveries). Boxed: an `Operation` dwarfs the `Ack` variant
    /// (clippy `large_enum_variant`); `Box<T>` serializes transparently.
    Draft(Box<Operation>),
}

impl AppliedOutcome {
    /// Extract the command ack a [`Reserved::Return`] carries. The op-name guard
    /// makes a variant mismatch impossible in practice (a key that stored an
    /// `Ack` can only be replayed under the same command op); a mismatch is an
    /// internal invariant break, surfaced as a `Conflict` rather than a panic.
    pub(crate) fn into_ack(self) -> Result<CommandAck, RuntimeError> {
        match self {
            AppliedOutcome::Ack(ack) => Ok(ack),
            AppliedOutcome::Draft(_) => Err(Self::variant_mismatch()),
        }
    }

    /// Extract the draft operation a [`Reserved::Return`] carries (see
    /// [`into_ack`](Self::into_ack) for the mismatch discipline).
    pub(crate) fn into_draft(self) -> Result<Operation, RuntimeError> {
        match self {
            AppliedOutcome::Draft(operation) => Ok(*operation),
            AppliedOutcome::Ack(_) => Err(Self::variant_mismatch()),
        }
    }

    fn variant_mismatch() -> RuntimeError {
        RuntimeError::new(
            RuntimeErrorCode::Conflict,
            "idempotency key reused with a different operation",
        )
    }
}

/// The durable state of a keyed apply decision, as the [`DurableApplyStore`]
/// records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurableApplyState {
    /// Reserved before execution; no outcome recorded yet. Found stale (from a
    /// previous process incarnation) it means "outcome unknown — crash between
    /// apply and record"; the ledger conservatively refuses to re-execute.
    Pending,
    /// Applied; the payload is a serialized [`AppliedOutcome`].
    Confirmed,
    /// Permanently rejected; the payload is the serialized rejection envelope.
    Rejected,
}

/// A durable apply decision read back on a duplicate reservation.
#[derive(Clone, Debug)]
pub struct DurableApplyRecord {
    /// The canonical op-name stored at reservation (the same-key-different-op
    /// guard, D128).
    pub op_name: String,
    pub state: DurableApplyState,
    /// The serialized decision payload; `None` while `Pending`.
    pub payload_json: Option<String>,
}

/// The verdict of [`DurableApplyStore::reserve`].
#[derive(Debug)]
pub enum DurableReserve {
    /// First durable sight of the key: a `pending` marker was written; the
    /// caller executes and then settles (or clears).
    Reserved,
    /// The key already has a durable record the caller must honor without
    /// executing.
    Existing(DurableApplyRecord),
}

/// The durable backing of the apply ledger (DS7): a keyed, restart-surviving
/// record of applied operations. `reserve` must be atomic (lookup + pending
/// insert in one transaction) so two racing processes — or a crash between
/// lookup and insert — cannot both reserve. Implemented over the authority
/// server's SQLite store (`posthaste-store` `apply_ledger` table) by the
/// co-located build; a store-less remote near node wires none and keeps the
/// pre-existing in-memory-only behavior.
///
/// Retention contract: settled records must outlive every realistic
/// redelivery window (the implementation keeps them for 30 days, dominating
/// the in-memory 15-minute TTL and webhook/agent retry horizons of
/// hours–days); `pending` records must never be reaped.
#[async_trait]
pub trait DurableApplyStore: Send + Sync {
    /// Atomically return the existing record for `(scope, key)` or insert a
    /// `pending` reservation stamped `now_secs`.
    async fn reserve(
        &self,
        scope: &str,
        key: &str,
        op_name: &str,
        now_secs: u64,
    ) -> Result<DurableReserve, RuntimeError>;

    /// Record the terminal decision for a reserved key. A missing row is a
    /// no-op.
    async fn settle(
        &self,
        scope: &str,
        key: &str,
        state: DurableApplyState,
        payload_json: &str,
        now_secs: u64,
    ) -> Result<(), RuntimeError>;

    /// Drop the record — a transient failure, so a deliberate retry
    /// re-reserves and re-executes (D47's `Failed` rule).
    async fn clear(&self, scope: &str, key: &str) -> Result<(), RuntimeError>;
}

/// The durable serialization of a Confirmed [`AppliedOutcome`]. Tagged so the
/// stored family survives round-tripping (the op-name guard already prevents a
/// cross-family replay; the tag keeps the payload self-describing).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StoredOutcome {
    Ack(CommandAck),
    // Boxed for parity with `AppliedOutcome` (`Box<T>` is serde-transparent,
    // so stored rows round-trip identically).
    Draft(Box<Operation>),
}

impl From<&AppliedOutcome> for StoredOutcome {
    fn from(outcome: &AppliedOutcome) -> Self {
        match outcome {
            AppliedOutcome::Ack(ack) => StoredOutcome::Ack(ack.clone()),
            AppliedOutcome::Draft(operation) => StoredOutcome::Draft(operation.clone()),
        }
    }
}

impl From<StoredOutcome> for AppliedOutcome {
    fn from(stored: StoredOutcome) -> Self {
        match stored {
            StoredOutcome::Ack(ack) => AppliedOutcome::Ack(ack),
            StoredOutcome::Draft(operation) => AppliedOutcome::Draft(operation),
        }
    }
}

/// Wall-clock seconds — the tick the ledger's TTL is measured against, matching
/// the units the far-end sink reaper drives [`DedupStore::reap`] on.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The per-caller idempotency bucket — the `LinkId` half of the ledger key. A
/// link-scoped caller keys per link; a keyless REST direct-apply caller
/// (`RuntimeCaller::api()`) shares one bucket per operation source. Multi-tenant
/// per-token scoping rides the token identity when that lands (post-slice-1);
/// for the single-user local milestone the source bucket is the caller scope.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct ApplyScope(String);

impl ApplyScope {
    fn of(caller: &RuntimeCaller) -> Self {
        match &caller.link_id {
            Some(id) => Self(format!("link:{}", id.as_str())),
            None => Self(format!("src:{:?}", caller.operation_source)),
        }
    }

    /// The stable string form the durable store keys on — the same value the
    /// in-memory ledger buckets by, so cache and durable store agree.
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stored direct-apply outcome, keyed by `(ApplyScope, ClientMutationId)`. The
/// operation name is retained so a same-key-different-op reuse is caught.
#[derive(Clone)]
struct AppliedRecord {
    /// The canonical operation name at reservation, for same-key-different-op
    /// detection (one fact, derived from the [`MailOperation`] variant).
    op_name: &'static str,
    /// The terminal outcome: `Ok` (Confirmed) or `Err` (Rejected). `None` while
    /// the reserved apply is still in flight (a concurrent duplicate).
    outcome: Option<Result<AppliedOutcome, RuntimeAdapterError>>,
}

/// The reservation verdict for one `(caller, key, op)`.
pub(crate) enum Reserved {
    /// First sight of this key: the caller must run the op, then [`settle`] it.
    ///
    /// [`settle`]: ApplyLedger::settle
    Execute,
    /// A prior outcome (or a conflict) to return WITHOUT executing. The `Ok`
    /// payload is boxed: [`AppliedOutcome`] is large (an `Ack`/`Draft` union) and
    /// would otherwise dominate this two-variant enum's size.
    Return(Result<Box<AppliedOutcome>, RuntimeError>),
}

/// The apply-scoped dedup ledger held on the runtime core (one instance behind
/// the cloneable [`RuntimeHandle`](crate::RuntimeHandle)). The in-memory
/// [`DedupStore`] is the in-flight guard + fast cache; `durable`, when wired,
/// is the restart/TTL-surviving source of truth for terminal decisions (DS7).
pub(crate) struct ApplyLedger {
    dedup: DedupStore<ApplyScope, AppliedRecord>,
    durable: Option<Arc<dyn DurableApplyStore>>,
}

impl ApplyLedger {
    /// An in-memory-only ledger — the store-less (remote near node) build, and
    /// the pre-DS7 behavior baseline the unit tests pin.
    pub(crate) fn new() -> Self {
        Self {
            dedup: DedupStore::new(),
            durable: None,
        }
    }

    /// A ledger whose terminal decisions are durably recorded in (and re-read
    /// from) `durable` — the co-located build's configuration.
    pub(crate) fn with_durable(durable: Arc<dyn DurableApplyStore>) -> Self {
        Self {
            dedup: DedupStore::new(),
            durable: Some(durable),
        }
    }

    /// Reserve `(caller, key)` for `op_name`. On [`Reserved::Execute`] the caller
    /// applies the op and calls [`settle`](Self::settle) with the result; on
    /// [`Reserved::Return`] it returns the carried result without executing.
    /// Opportunistically reaps stale in-memory terminals first (safe now that
    /// every terminal is durably recorded — a reaped decision is re-read from
    /// the durable store, not lost). A key that misses the in-memory cache is
    /// checked against the durable store, so a redelivery after a TTL reap or a
    /// process restart still finds the prior decision.
    pub(crate) async fn reserve(
        &self,
        caller: &RuntimeCaller,
        key: &ClientMutationId,
        op_name: &'static str,
    ) -> Reserved {
        let scope = ApplyScope::of(caller);
        let now = now_secs();
        self.dedup.reap(now);
        match self.dedup.accept(&scope, key, || AppliedRecord {
            op_name,
            outcome: None,
        }) {
            // In-memory miss: the durable store decides (it may still hold a
            // decision the cache reaped, or one from a previous process).
            Accept::New => self.reserve_durable(&scope, key, op_name, now).await,
            Accept::Duplicate(record) => {
                if record.op_name != op_name {
                    return Reserved::Return(Err(RuntimeError::new(
                        RuntimeErrorCode::Conflict,
                        "idempotency key reused with a different operation",
                    )));
                }
                match record.outcome {
                    Some(Ok(outcome)) => Reserved::Return(Ok(Box::new(outcome))),
                    Some(Err(error)) => Reserved::Return(Err(RuntimeError(error))),
                    None => Reserved::Return(Err(RuntimeError::retryable(
                        RuntimeErrorCode::Conflict,
                        "a mutation with this idempotency key is still in flight",
                    ))),
                }
            }
        }
    }

    /// The durable half of [`reserve`](Self::reserve), entered on an in-memory
    /// miss (the pending in-memory slot is already held; every non-`Execute`
    /// path must settle or clear it). With no durable store this is the
    /// pre-DS7 `Execute`.
    async fn reserve_durable(
        &self,
        scope: &ApplyScope,
        key: &ClientMutationId,
        op_name: &'static str,
        now: u64,
    ) -> Reserved {
        let Some(durable) = &self.durable else {
            return Reserved::Execute;
        };
        match durable
            .reserve(scope.as_str(), key.as_str(), op_name, now)
            .await
        {
            Ok(DurableReserve::Reserved) => Reserved::Execute,
            Ok(DurableReserve::Existing(record)) => {
                if record.op_name != op_name {
                    self.dedup.clear(scope, key);
                    return Reserved::Return(Err(RuntimeError::new(
                        RuntimeErrorCode::Conflict,
                        "idempotency key reused with a different operation",
                    )));
                }
                match record.state {
                    // A stale durable `pending` is a crash between apply and
                    // record: the outcome is unknown, so conservatively refuse
                    // to re-execute (for exactly-once send, not re-sending
                    // dominates). Permanent: retrying the same key cannot
                    // resolve the ambiguity — a caller that intends a NEW
                    // operation must use a new key.
                    DurableApplyState::Pending => {
                        self.dedup.clear(scope, key);
                        Reserved::Return(Err(RuntimeError::new(
                            RuntimeErrorCode::Conflict,
                            "a previous apply under this idempotency key never recorded \
                             its outcome (interrupted mid-apply); refusing to re-execute — \
                             use a new idempotency key to run a new operation",
                        )))
                    }
                    DurableApplyState::Confirmed => {
                        match record
                            .payload_json
                            .as_deref()
                            .map(serde_json::from_str::<StoredOutcome>)
                        {
                            Some(Ok(stored)) => {
                                let outcome = AppliedOutcome::from(stored);
                                self.cache_terminal(
                                    scope,
                                    key,
                                    TerminalClass::Confirmed,
                                    Ok(outcome.clone()),
                                    now,
                                );
                                Reserved::Return(Ok(Box::new(outcome)))
                            }
                            _ => self.corrupt_durable_decision(scope, key),
                        }
                    }
                    DurableApplyState::Rejected => {
                        match record
                            .payload_json
                            .as_deref()
                            .map(serde_json::from_str::<RuntimeAdapterError>)
                        {
                            Some(Ok(envelope)) => {
                                self.cache_terminal(
                                    scope,
                                    key,
                                    TerminalClass::Rejected,
                                    Err(envelope.clone()),
                                    now,
                                );
                                Reserved::Return(Err(RuntimeError(envelope)))
                            }
                            _ => self.corrupt_durable_decision(scope, key),
                        }
                    }
                }
            }
            // Fail CLOSED: with the durable ledger unreachable, executing
            // would risk a double-apply the moment durability returns (the
            // decision could not be recorded). Retryable — the store may
            // recover, and the in-memory slot is released so the retry can
            // re-reserve.
            Err(error) => {
                self.dedup.clear(scope, key);
                tracing::warn!(
                    error = %error.envelope().message,
                    "durable apply ledger unavailable on reserve; refusing to execute"
                );
                Reserved::Return(Err(RuntimeError::retryable(
                    RuntimeErrorCode::StorageFailure,
                    "the durable idempotency ledger is unavailable; \
                     the operation was not executed — retry",
                )))
            }
        }
    }

    /// Re-populate the in-memory cache with a durable terminal decision (the
    /// pending slot reserved by `accept` is settled in place), so subsequent
    /// redeliveries in this process hit the cache.
    fn cache_terminal(
        &self,
        scope: &ApplyScope,
        key: &ClientMutationId,
        class: TerminalClass,
        outcome: Result<AppliedOutcome, RuntimeAdapterError>,
        now: u64,
    ) {
        self.dedup
            .settle(scope, key, class, None, now, move |record| {
                record.outcome = Some(outcome)
            });
    }

    /// A durable decision that cannot be decoded: conservatively refuse to
    /// re-execute (the operation DID settle once — re-running risks a
    /// double-apply; only the stored payload is unusable).
    fn corrupt_durable_decision(&self, scope: &ApplyScope, key: &ClientMutationId) -> Reserved {
        self.dedup.clear(scope, key);
        tracing::warn!("durable apply decision for an idempotency key could not be decoded");
        Reserved::Return(Err(RuntimeError::new(
            RuntimeErrorCode::Conflict,
            "this idempotency key settled previously but its stored outcome \
             is unreadable; refusing to re-execute",
        )))
    }

    /// Record the outcome of an executed reservation under the D47 rule: `Ok` is
    /// kept `Confirmed`, a permanent error is kept `Rejected` (re-observed), a
    /// transient error clears the record so a deliberate retry re-executes.
    ///
    /// The durable record is written FIRST (the reservation already left a
    /// durable `pending` marker before the apply, so a crash anywhere in this
    /// window leaves `pending` — resolved conservatively as "outcome unknown,
    /// do not re-execute", never as a double-apply). A durable-write failure is
    /// logged and the in-memory settle still proceeds: the apply already
    /// happened, in-process redeliveries keep deduping off the cache, and the
    /// durable row stays `pending` (post-restart redeliveries refuse to
    /// re-execute rather than double-apply).
    ///
    /// The `Ok` side is taken by value (the caller clones the outcome payload it
    /// still owns to return to its own caller); the `Err` side is borrowed so no
    /// `RuntimeError` clone is forced on the hot path (only the envelope of a
    /// *permanent* error is cloned, to retain it for replay).
    pub(crate) async fn settle(
        &self,
        caller: &RuntimeCaller,
        key: &ClientMutationId,
        result: Result<AppliedOutcome, &RuntimeError>,
    ) {
        let scope = ApplyScope::of(caller);
        let now = now_secs();
        match result {
            Ok(outcome) => {
                self.settle_durable(
                    &scope,
                    key,
                    DurableApplyState::Confirmed,
                    serde_json::to_string(&StoredOutcome::from(&outcome)).ok(),
                    now,
                )
                .await;
                self.dedup.settle(
                    &scope,
                    key,
                    TerminalClass::Confirmed,
                    None,
                    now,
                    move |record| record.outcome = Some(Ok(outcome)),
                );
            }
            Err(error) if error.envelope().terminality == Terminality::Permanent => {
                let error = error.envelope().clone();
                self.settle_durable(
                    &scope,
                    key,
                    DurableApplyState::Rejected,
                    serde_json::to_string(&error).ok(),
                    now,
                )
                .await;
                self.dedup.settle(
                    &scope,
                    key,
                    TerminalClass::Rejected,
                    None,
                    now,
                    move |record| record.outcome = Some(Err(error)),
                );
            }
            Err(_) => {
                // Transient: clear durably too, so a deliberate retry
                // re-executes even after a restart (D47's `Failed` rule,
                // preserved across the durability boundary).
                if let Some(durable) = &self.durable {
                    if let Err(error) = durable.clear(scope.as_str(), key.as_str()).await {
                        tracing::warn!(
                            error = %error.envelope().message,
                            "durable apply ledger clear failed after a transient error; \
                             a post-restart retry under this key will be refused \
                             (stale pending) — retry with a new key if needed"
                        );
                    }
                }
                self.dedup.clear(&scope, key);
            }
        }
    }

    /// Best-effort durable settle (see [`settle`](Self::settle) for the
    /// failure discipline). `payload_json: None` means the payload failed to
    /// serialize — vanishingly unlikely for these types; the row is then left
    /// `pending` (conservative) rather than settled with a lie.
    async fn settle_durable(
        &self,
        scope: &ApplyScope,
        key: &ClientMutationId,
        state: DurableApplyState,
        payload_json: Option<String>,
        now: u64,
    ) {
        let Some(durable) = &self.durable else {
            return;
        };
        let Some(payload_json) = payload_json else {
            tracing::warn!("apply outcome failed to serialize; durable record left pending");
            return;
        };
        if let Err(error) = durable
            .settle(scope.as_str(), key.as_str(), state, &payload_json, now)
            .await
        {
            tracing::warn!(
                error = %error.envelope().message,
                "durable apply ledger settle failed; record left pending \
                 (a post-restart redelivery will be refused, not re-executed)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use posthaste_contract_core::mutation_args::MessageTargetArgs;
    use posthaste_contract_core::MailOperation;
    use posthaste_domain_model::{
        AccountId, CommandAck, Operation, OperationEntity, OperationEntityKind, OperationId,
        OperationKind, OperationState,
    };

    fn caller() -> RuntimeCaller {
        RuntimeCaller::api()
    }
    fn key(s: &str) -> ClientMutationId {
        ClientMutationId::new(s)
    }
    fn ack_outcome() -> AppliedOutcome {
        AppliedOutcome::Ack(CommandAck { events: vec![] })
    }
    fn draft_outcome(id: &str) -> AppliedOutcome {
        AppliedOutcome::Draft(Box::new(Operation {
            id: OperationId::from(id),
            account_id: AccountId("acct".into()),
            entity: OperationEntity {
                kind: OperationEntityKind::Draft,
                id: "draft-1".into(),
            },
            kind: OperationKind::DraftCreate,
            payload: serde_json::json!({}),
            payload_version: 1,
            state: OperationState::Pending,
            attempts: 0,
            last_error: None,
            depends_on: None,
            send_at: None,
            hold_until_mono: None,
            created_at: "1970-01-01T00:00:00Z".to_string(),
            updated_at: "1970-01-01T00:00:00Z".to_string(),
        }))
    }
    fn destroy() -> &'static str {
        MailOperation::Destroy(MessageTargetArgs {
            source_id: "a".into(),
            message_id: "m".into(),
        })
        .name()
    }
    // The send op name the send route reserves under (RFC-L2-scripting ruling
    // 24). Mirrors `handle::SEND_OP_NAME`; kept in sync as the ledger's op-name
    // slot. A send settles an `Ack`-shaped outcome (it produces no `Operation`).
    fn send() -> &'static str {
        "message.send"
    }

    /// An in-memory stand-in for the SQLite `apply_ledger` table: a keyed map
    /// that OUTLIVES any [`ApplyLedger`] built over it, so dropping the ledger
    /// and building a fresh one over the same store simulates a process
    /// restart (and, equivalently, an in-memory TTL reap — both lose only the
    /// in-memory map). `fail` makes every call error, for the fail-closed test.
    /// One stored row of the fake: `(op_name, state, payload_json)`.
    type MemoryRow = (String, DurableApplyState, Option<String>);

    #[derive(Default)]
    struct MemoryDurable {
        rows: Mutex<HashMap<(String, String), MemoryRow>>,
        fail: std::sync::atomic::AtomicBool,
    }

    impl MemoryDurable {
        fn check(&self) -> Result<(), RuntimeError> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(RuntimeError::new(
                    RuntimeErrorCode::StorageFailure,
                    "durable store down",
                ));
            }
            Ok(())
        }
    }

    #[async_trait]
    impl DurableApplyStore for MemoryDurable {
        async fn reserve(
            &self,
            scope: &str,
            key: &str,
            op_name: &str,
            _now_secs: u64,
        ) -> Result<DurableReserve, RuntimeError> {
            self.check()?;
            let mut rows = self.rows.lock().unwrap();
            if let Some((op_name, state, payload)) = rows.get(&(scope.to_string(), key.to_string()))
            {
                return Ok(DurableReserve::Existing(DurableApplyRecord {
                    op_name: op_name.clone(),
                    state: *state,
                    payload_json: payload.clone(),
                }));
            }
            rows.insert(
                (scope.to_string(), key.to_string()),
                (op_name.to_string(), DurableApplyState::Pending, None),
            );
            Ok(DurableReserve::Reserved)
        }

        async fn settle(
            &self,
            scope: &str,
            key: &str,
            state: DurableApplyState,
            payload_json: &str,
            _now_secs: u64,
        ) -> Result<(), RuntimeError> {
            self.check()?;
            let mut rows = self.rows.lock().unwrap();
            if let Some(row) = rows.get_mut(&(scope.to_string(), key.to_string())) {
                row.1 = state;
                row.2 = Some(payload_json.to_string());
            }
            Ok(())
        }

        async fn clear(&self, scope: &str, key: &str) -> Result<(), RuntimeError> {
            self.check()?;
            self.rows
                .lock()
                .unwrap()
                .remove(&(scope.to_string(), key.to_string()));
            Ok(())
        }
    }

    // A redelivery under the same key returns the first Confirmed outcome and
    // never re-executes (the milestone's write-back-dedup property).
    #[tokio::test]
    async fn confirmed_is_re_observed_on_redelivery() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        assert!(matches!(
            ledger.reserve(&c, &k, destroy()).await,
            Reserved::Execute
        ));
        ledger.settle(&c, &k, Ok(ack_outcome())).await;
        match ledger.reserve(&c, &k, destroy()).await {
            Reserved::Return(Ok(_)) => {}
            _ => panic!("a redelivery must re-observe the stored Confirmed, not execute"),
        }
    }

    // The draft routes settle an operation-bearing outcome (D128): a redelivery
    // under the same key re-observes the SAME operation, id and all, so the
    // response body is byte-identical and no second draft is enqueued.
    #[tokio::test]
    async fn draft_outcome_replays_the_same_operation_id() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("save-1"));
        assert!(matches!(
            ledger.reserve(&c, &k, "draft.save").await,
            Reserved::Execute
        ));
        ledger.settle(&c, &k, Ok(draft_outcome("op-abc"))).await;
        match ledger.reserve(&c, &k, "draft.save").await {
            Reserved::Return(result) => {
                let operation = (*result.unwrap()).into_draft().expect("a draft outcome");
                assert_eq!(operation.id, OperationId::from("op-abc"));
            }
            Reserved::Execute => panic!("a replayed save must re-observe the stored operation"),
        }
    }

    // A stored draft outcome extracted as an ack (the wrong family) is an
    // internal invariant break, surfaced as Conflict rather than a panic. The
    // op-name guard makes this unreachable in production; the check is defensive.
    #[test]
    fn outcome_variant_mismatch_is_a_conflict_not_a_panic() {
        assert_eq!(
            draft_outcome("op-abc")
                .into_ack()
                .unwrap_err()
                .envelope()
                .code,
            RuntimeErrorCode::Conflict
        );
        assert_eq!(
            ack_outcome().into_draft().unwrap_err().envelope().code,
            RuntimeErrorCode::Conflict
        );
    }

    // A permanent rejection is kept and re-observed; a redelivery never re-runs.
    #[tokio::test]
    async fn rejected_is_re_observed_on_redelivery() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, destroy()).await;
        ledger
            .settle(
                &c,
                &k,
                Err(&RuntimeError::new(RuntimeErrorCode::InvalidMutation, "no")),
            )
            .await;
        match ledger.reserve(&c, &k, destroy()).await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::InvalidMutation)
            }
            _ => panic!("a rejected redelivery must re-observe the rejection"),
        }
    }

    // A transient failure clears the record so a deliberate retry re-executes.
    #[tokio::test]
    async fn transient_failure_clears_and_re_executes() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, destroy()).await;
        ledger
            .settle(
                &c,
                &k,
                Err(&RuntimeError::retryable(
                    RuntimeErrorCode::TransportDisconnected,
                    "down",
                )),
            )
            .await;
        assert!(matches!(
            ledger.reserve(&c, &k, destroy()).await,
            Reserved::Execute
        ));
    }

    // Ruling 24 (sequential): a keyed send re-runs under the SAME key → the
    // ledger returns the first outcome, NOT a second `Execute`. Because a second
    // `Execute` is what would call `authority_server_link.send_message` again,
    // `Return` here is exactly "no second outbox send is enqueued".
    #[tokio::test]
    async fn keyed_send_redelivery_returns_first_outcome_no_second_execute() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("send-1"));
        assert!(
            matches!(ledger.reserve(&c, &k, send()).await, Reserved::Execute),
            "first sight of the send key must execute"
        );
        ledger.settle(&c, &k, Ok(ack_outcome())).await;
        match ledger.reserve(&c, &k, send()).await {
            Reserved::Return(result) => {
                result.expect("a redelivered send re-observes the Confirmed outcome");
            }
            Reserved::Execute => {
                panic!("a redelivered send must re-observe the first outcome, not re-send")
            }
        }
    }

    // Ruling 24 (concurrent duplicate): two simultaneous sends under the same key
    // — the second reserving BEFORE the first settles — yield exactly one
    // `Execute`; the in-flight duplicate gets a retryable `Conflict`, never a
    // second send. This reuses the ledger's existing reservation guarantee, not a
    // new mechanism.
    #[tokio::test]
    async fn concurrent_duplicate_send_reserves_exactly_one_execute() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("send-race"));
        let first = ledger.reserve(&c, &k, send()).await;
        let second = ledger.reserve(&c, &k, send()).await;
        assert!(
            matches!(first, Reserved::Execute),
            "exactly one of the racing sends executes"
        );
        match second {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict);
                assert!(
                    error.envelope().terminality != Terminality::Permanent,
                    "an in-flight duplicate is retryable, not a permanent verdict"
                );
            }
            _ => panic!("the concurrent duplicate must not also execute (double-send)"),
        }
    }

    // Scheduled sends: a keyed send stores its enqueued OPERATION, so a
    // redelivery re-observes the SAME operation — id (the cancel handle) and
    // all — never enqueuing a second scheduled send.
    #[tokio::test]
    async fn keyed_send_replay_returns_the_same_operation_id() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("send-sched"));
        assert!(matches!(
            ledger.reserve(&c, &k, send()).await,
            Reserved::Execute
        ));
        ledger.settle(&c, &k, Ok(draft_outcome("op-send-1"))).await;
        match ledger.reserve(&c, &k, send()).await {
            Reserved::Return(result) => {
                let operation = (*result.unwrap()).into_draft().expect("operation outcome");
                assert_eq!(operation.id, OperationId::from("op-send-1"));
            }
            Reserved::Execute => panic!("a replayed send must re-observe the stored operation"),
        }
    }

    // Undo-send cancel interaction: the ledger key settled Confirmed at
    // ENQUEUE, and canceling (discarding) the outbox operation later does NOT
    // un-settle it — a retry under the same key re-observes the original
    // outcome and never creates (or sends) a second operation. The outbox row
    // is gone; the ledger's terminal record is the guarantee that the key can
    // never re-execute.
    #[tokio::test]
    async fn canceled_scheduled_send_key_stays_terminally_settled() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("send-undone"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            ledger.reserve(&c, &k, send()).await;
            ledger.settle(&c, &k, Ok(draft_outcome("op-undone"))).await;
            // ... the user cancels: the OUTBOX op is discarded. Nothing here —
            // the ledger record is deliberately untouched by a cancel.
        }
        // Same-process and post-restart replays both re-observe, never execute.
        let reborn = ApplyLedger::with_durable(durable);
        match reborn.reserve(&c, &k, send()).await {
            Reserved::Return(result) => {
                let operation = (*result.unwrap()).into_draft().expect("operation outcome");
                assert_eq!(operation.id, OperationId::from("op-undone"));
            }
            Reserved::Execute => {
                panic!("a canceled send's key must never re-execute (no resurrection)")
            }
        }
    }

    // Ruling 24 (distinct keys): two sends under DIFFERENT keys each execute —
    // two operations, as intended (the header only dedupes an identical key).
    #[tokio::test]
    async fn distinct_send_keys_each_execute() {
        let ledger = ApplyLedger::new();
        let c = caller();
        let (k1, k2) = (key("send-a"), key("send-b"));
        assert!(matches!(
            ledger.reserve(&c, &k1, send()).await,
            Reserved::Execute
        ));
        ledger.settle(&c, &k1, Ok(ack_outcome())).await;
        assert!(
            matches!(ledger.reserve(&c, &k2, send()).await, Reserved::Execute),
            "a different key is a different send and must execute"
        );
    }

    // A send key reused for a message-command (or vice versa) is a `Conflict`:
    // `SEND_OP_NAME` is distinct from every `MailOperation::name()`.
    #[tokio::test]
    async fn send_key_reused_for_a_message_command_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("shared-key"));
        ledger.reserve(&c, &k, send()).await;
        ledger.settle(&c, &k, Ok(ack_outcome())).await;
        match ledger.reserve(&c, &k, destroy()).await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a send key reused with a message-command op must be rejected"),
        }
    }

    // A send key reused for a draft op is likewise a `Conflict`: `SEND_OP_NAME`
    // is distinct from `draft.save`/`draft.delete` too.
    #[tokio::test]
    async fn send_key_reused_for_a_draft_op_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("shared-key"));
        ledger.reserve(&c, &k, send()).await;
        ledger.settle(&c, &k, Ok(ack_outcome())).await;
        match ledger.reserve(&c, &k, DRAFT_SAVE_OP).await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a send key reused with a draft op must be rejected"),
        }
    }

    // Same key, different op → rejected (the replica path's rule).
    #[tokio::test]
    async fn same_key_different_op_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, "message.setKeywords").await;
        ledger.settle(&c, &k, Ok(ack_outcome())).await;
        match ledger.reserve(&c, &k, "message.destroy").await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a same-key-different-op reuse must be rejected"),
        }
    }

    // ------------------------------------------------------------------
    // DS7 (mail-safety durability): the durable ledger survives what the
    // in-memory map cannot — a TTL reap and a process restart.
    // ------------------------------------------------------------------

    // THE DS7 property: apply op X → lose the in-memory ledger (a restart,
    // and equivalently a TTL reap — both lose only the in-memory map) →
    // redeliver op X under the same key → NOT re-executed: the durable
    // decision is found and re-observed.
    #[tokio::test]
    async fn restart_redelivery_re_observes_the_durable_decision_never_re_executes() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("send-1"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            assert!(matches!(
                ledger.reserve(&c, &k, send()).await,
                Reserved::Execute
            ));
            ledger.settle(&c, &k, Ok(ack_outcome())).await;
        } // ← the in-memory ledger dies here (restart / reap).
        let reborn = ApplyLedger::with_durable(durable);
        match reborn.reserve(&c, &k, send()).await {
            Reserved::Return(result) => {
                result
                    .expect("the durable Confirmed decision is re-observed")
                    .into_ack()
                    .expect("a send stores an Ack-shaped outcome");
            }
            Reserved::Execute => {
                panic!("a post-restart redelivery must NOT re-execute (double-send)")
            }
        }
    }

    // The durable outcome payload round-trips: a draft decision replayed after
    // a restart yields the SAME operation id (D128's byte-identical response),
    // not merely "some prior success".
    #[tokio::test]
    async fn restart_redelivery_replays_the_same_draft_operation_id() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("save-1"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            ledger.reserve(&c, &k, DRAFT_SAVE_OP).await;
            ledger.settle(&c, &k, Ok(draft_outcome("op-abc"))).await;
        }
        let reborn = ApplyLedger::with_durable(durable);
        match reborn.reserve(&c, &k, DRAFT_SAVE_OP).await {
            Reserved::Return(result) => {
                let operation = (*result.unwrap()).into_draft().expect("a draft outcome");
                assert_eq!(operation.id, OperationId::from("op-abc"));
            }
            Reserved::Execute => panic!("a post-restart replayed save must not enqueue again"),
        }
    }

    // A never-seen key still executes with the durable store wired — the
    // durability fix must not block first-sight operations.
    #[tokio::test]
    async fn never_seen_key_still_executes_with_durable_store() {
        let ledger = ApplyLedger::with_durable(Arc::new(MemoryDurable::default()));
        assert!(matches!(
            ledger.reserve(&caller(), &key("fresh"), send()).await,
            Reserved::Execute
        ));
    }

    // A permanent rejection survives the restart too: the same verdict is
    // re-observed, never re-executed.
    #[tokio::test]
    async fn restart_redelivery_re_observes_a_durable_rejection() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("op-1"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            ledger.reserve(&c, &k, destroy()).await;
            ledger
                .settle(
                    &c,
                    &k,
                    Err(&RuntimeError::new(RuntimeErrorCode::InvalidMutation, "no")),
                )
                .await;
        }
        let reborn = ApplyLedger::with_durable(durable);
        match reborn.reserve(&c, &k, destroy()).await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::InvalidMutation)
            }
            _ => panic!("a durable rejection must be re-observed after restart"),
        }
    }

    // A transient failure clears the DURABLE record too, so a deliberate retry
    // re-executes even across a restart (D47's Failed rule, preserved).
    #[tokio::test]
    async fn transient_failure_clears_durably_so_a_post_restart_retry_re_executes() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("op-1"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            ledger.reserve(&c, &k, destroy()).await;
            ledger
                .settle(
                    &c,
                    &k,
                    Err(&RuntimeError::retryable(
                        RuntimeErrorCode::TransportDisconnected,
                        "down",
                    )),
                )
                .await;
        }
        let reborn = ApplyLedger::with_durable(durable);
        assert!(matches!(
            reborn.reserve(&c, &k, destroy()).await,
            Reserved::Execute
        ));
    }

    // Crash-between-apply-and-record cannot double-apply: the durable `pending`
    // marker is written BEFORE the apply executes and only leaves `pending` on
    // settle — so a crash anywhere between apply and record leaves the marker,
    // and the redelivery is refused (permanent Conflict, "outcome unknown"),
    // never re-executed. This is the record-atomic-with-apply assertion: there
    // is no window in which the operation ran without a durable trace.
    #[tokio::test]
    async fn crash_between_apply_and_record_refuses_redelivery_never_double_applies() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("send-1"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            assert!(matches!(
                ledger.reserve(&c, &k, send()).await,
                Reserved::Execute
            ));
            // The apply runs here... and the process crashes before `settle`.
        }
        let reborn = ApplyLedger::with_durable(durable);
        match reborn.reserve(&c, &k, send()).await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict);
                assert_eq!(
                    error.envelope().terminality,
                    Terminality::Permanent,
                    "outcome-unknown is permanent: same-key retries cannot resolve it"
                );
            }
            Reserved::Execute => panic!("an unresolved pending must never re-execute"),
            Reserved::Return(Ok(_)) => panic!("no outcome was recorded; none can be returned"),
        }
    }

    // The op-name guard holds across the durability boundary: a key settled
    // under one op, replayed after restart under another, Conflicts.
    #[tokio::test]
    async fn durable_op_name_guard_survives_restart() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("shared-key"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            ledger.reserve(&c, &k, send()).await;
            ledger.settle(&c, &k, Ok(ack_outcome())).await;
        }
        let reborn = ApplyLedger::with_durable(durable);
        match reborn.reserve(&c, &k, destroy()).await {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a cross-op key reuse must be rejected after restart too"),
        }
    }

    // Fail CLOSED: with the durable ledger unreachable, the operation is NOT
    // executed (executing would risk an unrecorded apply — the DS7 bug
    // reintroduced); the caller gets a retryable storage error, and the key is
    // not poisoned — it executes once the store recovers.
    #[tokio::test]
    async fn unavailable_durable_store_fails_closed_and_recovers() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let ledger = ApplyLedger::with_durable(durable.clone());
        let (c, k) = (caller(), key("send-1"));
        durable
            .fail
            .store(true, std::sync::atomic::Ordering::SeqCst);
        match ledger.reserve(&c, &k, send()).await {
            Reserved::Return(Err(error)) => {
                assert!(
                    error.envelope().terminality != Terminality::Permanent,
                    "a durable outage is retryable, not a verdict"
                );
            }
            _ => panic!("with the durable ledger down, the send must not execute"),
        }
        durable
            .fail
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(
            matches!(ledger.reserve(&c, &k, send()).await, Reserved::Execute),
            "once the store recovers, the key executes normally"
        );
    }

    // The durable hit is re-cached: after a restart, the FIRST redelivery reads
    // the durable store and the second is served from the in-memory cache —
    // both re-observe the same outcome (a cache-consistency sanity check).
    #[tokio::test]
    async fn durable_hit_repopulates_the_in_memory_cache() {
        let durable: Arc<MemoryDurable> = Arc::new(MemoryDurable::default());
        let (c, k) = (caller(), key("send-1"));
        {
            let ledger = ApplyLedger::with_durable(durable.clone());
            ledger.reserve(&c, &k, send()).await;
            ledger.settle(&c, &k, Ok(ack_outcome())).await;
        }
        let reborn = ApplyLedger::with_durable(durable.clone());
        assert!(matches!(
            reborn.reserve(&c, &k, send()).await,
            Reserved::Return(Ok(_))
        ));
        // Second redelivery: served from the repopulated cache (the durable
        // store would also answer, but the cached path must agree).
        durable
            .fail
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            matches!(
                reborn.reserve(&c, &k, send()).await,
                Reserved::Return(Ok(_))
            ),
            "the repopulated cache serves the redelivery even with the store down"
        );
    }
}
