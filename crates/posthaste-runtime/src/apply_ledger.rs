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
//! (`settlement_seq = None`), so acked-cursor eviction never fires; the ledger is
//! bounded by the TTL and the safety-valve cap, reaped opportunistically on each
//! reservation (no background reaper runs on the direct-apply path).

use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_contract_core::{
    ClientMutationId, RuntimeAdapterError, RuntimeCaller, RuntimeError, RuntimeErrorCode,
    Terminality,
};
use posthaste_domain_model::{CommandAck, Operation};
use posthaste_link_far_end::up::{Accept, DedupStore, TerminalClass};

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
    /// destroy).
    Ack(CommandAck),
    /// An enqueued draft operation (`draft.save` / `draft.delete`).
    Draft(Operation),
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
            AppliedOutcome::Draft(operation) => Ok(operation),
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
/// the cloneable [`RuntimeHandle`](crate::RuntimeHandle)).
pub(crate) struct ApplyLedger {
    dedup: DedupStore<ApplyScope, AppliedRecord>,
}

impl ApplyLedger {
    pub(crate) fn new() -> Self {
        Self {
            dedup: DedupStore::new(),
        }
    }

    /// Reserve `(caller, key)` for `op_name`. On [`Reserved::Execute`] the caller
    /// applies the op and calls [`settle`](Self::settle) with the result; on
    /// [`Reserved::Return`] it returns the carried result without executing.
    /// Opportunistically reaps stale terminals first (no background reaper drives
    /// this path).
    pub(crate) fn reserve(
        &self,
        caller: &RuntimeCaller,
        key: &ClientMutationId,
        op_name: &'static str,
    ) -> Reserved {
        let scope = ApplyScope::of(caller);
        self.dedup.reap(now_secs());
        match self.dedup.accept(&scope, key, || AppliedRecord {
            op_name,
            outcome: None,
        }) {
            Accept::New => Reserved::Execute,
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

    /// Record the outcome of an executed reservation under the D47 rule: `Ok` is
    /// kept `Confirmed`, a permanent error is kept `Rejected` (re-observed), a
    /// transient error clears the record so a deliberate retry re-executes.
    ///
    /// The `Ok` side is taken by value (the caller clones the outcome payload it
    /// still owns to return to its own caller); the `Err` side is borrowed so no
    /// `RuntimeError` clone is forced on the hot path (only the envelope of a
    /// *permanent* error is cloned, to retain it for replay).
    pub(crate) fn settle(
        &self,
        caller: &RuntimeCaller,
        key: &ClientMutationId,
        result: Result<AppliedOutcome, &RuntimeError>,
    ) {
        let scope = ApplyScope::of(caller);
        match result {
            Ok(outcome) => {
                self.dedup.settle(
                    &scope,
                    key,
                    TerminalClass::Confirmed,
                    None,
                    now_secs(),
                    move |record| record.outcome = Some(Ok(outcome)),
                );
            }
            Err(error) if error.envelope().terminality == Terminality::Permanent => {
                let error = error.envelope().clone();
                self.dedup.settle(
                    &scope,
                    key,
                    TerminalClass::Rejected,
                    None,
                    now_secs(),
                    move |record| record.outcome = Some(Err(error)),
                );
            }
            Err(_) => self.dedup.clear(&scope, key),
        }
    }
}

#[cfg(test)]
mod tests {
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
        AppliedOutcome::Draft(Operation {
            id: OperationId::from(id),
            account_id: AccountId("acct".into()),
            entity: OperationEntity {
                kind: OperationEntityKind::Draft,
                id: "draft-1".into(),
            },
            kind: OperationKind::DraftCreate,
            payload: serde_json::json!({}),
            state: OperationState::Pending,
            attempts: 0,
            last_error: None,
            depends_on: None,
            created_at: "1970-01-01T00:00:00Z".to_string(),
            updated_at: "1970-01-01T00:00:00Z".to_string(),
        })
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

    // A redelivery under the same key returns the first Confirmed outcome and
    // never re-executes (the milestone's write-back-dedup property).
    #[test]
    fn confirmed_is_re_observed_on_redelivery() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        assert!(matches!(
            ledger.reserve(&c, &k, destroy()),
            Reserved::Execute
        ));
        ledger.settle(&c, &k, Ok(ack_outcome()));
        match ledger.reserve(&c, &k, destroy()) {
            Reserved::Return(Ok(_)) => {}
            _ => panic!("a redelivery must re-observe the stored Confirmed, not execute"),
        }
    }

    // The draft routes settle an operation-bearing outcome (D128): a redelivery
    // under the same key re-observes the SAME operation, id and all, so the
    // response body is byte-identical and no second draft is enqueued.
    #[test]
    fn draft_outcome_replays_the_same_operation_id() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("save-1"));
        assert!(matches!(
            ledger.reserve(&c, &k, "draft.save"),
            Reserved::Execute
        ));
        ledger.settle(&c, &k, Ok(draft_outcome("op-abc")));
        match ledger.reserve(&c, &k, "draft.save") {
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
    #[test]
    fn rejected_is_re_observed_on_redelivery() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, destroy());
        ledger.settle(
            &c,
            &k,
            Err(&RuntimeError::new(RuntimeErrorCode::InvalidMutation, "no")),
        );
        match ledger.reserve(&c, &k, destroy()) {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::InvalidMutation)
            }
            _ => panic!("a rejected redelivery must re-observe the rejection"),
        }
    }

    // A transient failure clears the record so a deliberate retry re-executes.
    #[test]
    fn transient_failure_clears_and_re_executes() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, destroy());
        ledger.settle(
            &c,
            &k,
            Err(&RuntimeError::retryable(
                RuntimeErrorCode::TransportDisconnected,
                "down",
            )),
        );
        assert!(matches!(
            ledger.reserve(&c, &k, destroy()),
            Reserved::Execute
        ));
    }

    // Ruling 24 (sequential): a keyed send re-runs under the SAME key → the
    // ledger returns the first outcome, NOT a second `Execute`. Because a second
    // `Execute` is what would call `authority_server_link.send_message` again,
    // `Return` here is exactly "no second outbox send is enqueued".
    #[test]
    fn keyed_send_redelivery_returns_first_outcome_no_second_execute() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("send-1"));
        assert!(
            matches!(ledger.reserve(&c, &k, send()), Reserved::Execute),
            "first sight of the send key must execute"
        );
        ledger.settle(&c, &k, Ok(ack_outcome()));
        match ledger.reserve(&c, &k, send()) {
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
    #[test]
    fn concurrent_duplicate_send_reserves_exactly_one_execute() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("send-race"));
        let first = ledger.reserve(&c, &k, send());
        let second = ledger.reserve(&c, &k, send());
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

    // Ruling 24 (distinct keys): two sends under DIFFERENT keys each execute —
    // two operations, as intended (the header only dedupes an identical key).
    #[test]
    fn distinct_send_keys_each_execute() {
        let ledger = ApplyLedger::new();
        let c = caller();
        let (k1, k2) = (key("send-a"), key("send-b"));
        assert!(matches!(ledger.reserve(&c, &k1, send()), Reserved::Execute));
        ledger.settle(&c, &k1, Ok(ack_outcome()));
        assert!(
            matches!(ledger.reserve(&c, &k2, send()), Reserved::Execute),
            "a different key is a different send and must execute"
        );
    }

    // A send key reused for a message-command (or vice versa) is a `Conflict`:
    // `SEND_OP_NAME` is distinct from every `MailOperation::name()`.
    #[test]
    fn send_key_reused_for_a_message_command_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("shared-key"));
        ledger.reserve(&c, &k, send());
        ledger.settle(&c, &k, Ok(ack_outcome()));
        match ledger.reserve(&c, &k, destroy()) {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a send key reused with a message-command op must be rejected"),
        }
    }

    // A send key reused for a draft op is likewise a `Conflict`: `SEND_OP_NAME`
    // is distinct from `draft.save`/`draft.delete` too.
    #[test]
    fn send_key_reused_for_a_draft_op_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("shared-key"));
        ledger.reserve(&c, &k, send());
        ledger.settle(&c, &k, Ok(ack_outcome()));
        match ledger.reserve(&c, &k, DRAFT_SAVE_OP) {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a send key reused with a draft op must be rejected"),
        }
    }

    // Same key, different op → rejected (the replica path's rule).
    #[test]
    fn same_key_different_op_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, "message.setKeywords");
        ledger.settle(&c, &k, Ok(ack_outcome()));
        match ledger.reserve(&c, &k, "message.destroy") {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a same-key-different-op reuse must be rejected"),
        }
    }
}
