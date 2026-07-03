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
use posthaste_domain_model::CommandAck;
use posthaste_link_far_end::up::{Accept, DedupStore, TerminalClass};

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
    outcome: Option<Result<CommandAck, RuntimeAdapterError>>,
}

/// The reservation verdict for one `(caller, key, op)`.
pub(crate) enum Reserved {
    /// First sight of this key: the caller must run the op, then [`settle`] it.
    ///
    /// [`settle`]: ApplyLedger::settle
    Execute,
    /// A prior outcome (or a conflict) to return WITHOUT executing.
    Return(Result<CommandAck, RuntimeError>),
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
                    Some(Ok(ack)) => Reserved::Return(Ok(ack)),
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
    pub(crate) fn settle(
        &self,
        caller: &RuntimeCaller,
        key: &ClientMutationId,
        result: &Result<CommandAck, RuntimeError>,
    ) {
        let scope = ApplyScope::of(caller);
        match result {
            Ok(ack) => {
                let ack = ack.clone();
                self.dedup.settle(
                    &scope,
                    key,
                    TerminalClass::Confirmed,
                    None,
                    now_secs(),
                    move |record| record.outcome = Some(Ok(ack)),
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
    use posthaste_domain_model::CommandAck;

    fn caller() -> RuntimeCaller {
        RuntimeCaller::api()
    }
    fn key(s: &str) -> ClientMutationId {
        ClientMutationId::new(s)
    }
    fn ack() -> CommandAck {
        CommandAck { events: vec![] }
    }
    fn destroy() -> &'static str {
        MailOperation::Destroy(MessageTargetArgs {
            source_id: "a".into(),
            message_id: "m".into(),
        })
        .name()
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
        ledger.settle(&c, &k, &Ok(ack()));
        match ledger.reserve(&c, &k, destroy()) {
            Reserved::Return(Ok(_)) => {}
            _ => panic!("a redelivery must re-observe the stored Confirmed, not execute"),
        }
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
            &Err(RuntimeError::new(RuntimeErrorCode::InvalidMutation, "no")),
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
            &Err(RuntimeError::retryable(
                RuntimeErrorCode::TransportDisconnected,
                "down",
            )),
        );
        assert!(matches!(
            ledger.reserve(&c, &k, destroy()),
            Reserved::Execute
        ));
    }

    // Same key, different op → rejected (the replica path's rule).
    #[test]
    fn same_key_different_op_is_rejected() {
        let ledger = ApplyLedger::new();
        let (c, k) = (caller(), key("op-1"));
        ledger.reserve(&c, &k, "message.setKeywords");
        ledger.settle(&c, &k, &Ok(ack()));
        match ledger.reserve(&c, &k, "message.destroy") {
            Reserved::Return(Err(error)) => {
                assert_eq!(error.envelope().code, RuntimeErrorCode::Conflict)
            }
            _ => panic!("a same-key-different-op reuse must be rejected"),
        }
    }
}
