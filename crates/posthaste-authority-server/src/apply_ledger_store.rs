//! The SQLite backing of the runtime's apply-scoped idempotency ledger (DS7 —
//! mail-safety durability): adapts the store's `apply_ledger` table
//! ([`posthaste_store`] `apply_ledger.rs`) to the runtime's
//! [`DurableApplyStore`] seam, so keyed direct-apply/send/draft decisions
//! survive the in-memory ledger's TTL reap and a process restart.
//!
//! Wired by the co-located build only ([`crate::build`]) — the runtime crate
//! itself stays store-free; this far-node crate, which owns the
//! [`DatabaseStore`], is where the two meet.

use std::sync::Arc;

use async_trait::async_trait;
use posthaste_contract_core::RuntimeError;
use posthaste_domain_model::StoreError;
use posthaste_runtime::{DurableApplyRecord, DurableApplyState, DurableApplyStore, DurableReserve};
use posthaste_store::{ApplyLedgerReserve, ApplyLedgerState, DatabaseStore};

/// [`DurableApplyStore`] over the authority server's [`DatabaseStore`]
/// `apply_ledger` table. Reservations are atomic (lookup + pending insert in
/// one write transaction); retention is the table's 30-day settled-row GC
/// (`posthaste_store::APPLY_LEDGER_RETENTION_SECS`), with `pending` crash
/// markers never reaped.
pub(crate) struct StoreDurableApplyLedger {
    store: Arc<DatabaseStore>,
}

impl StoreDurableApplyLedger {
    pub(crate) fn new(store: Arc<DatabaseStore>) -> Self {
        Self { store }
    }
}

fn store_error(error: StoreError) -> RuntimeError {
    RuntimeError::internal(format!("apply ledger store: {error}"), None)
}

fn to_store_state(state: DurableApplyState) -> ApplyLedgerState {
    match state {
        DurableApplyState::Pending => ApplyLedgerState::Pending,
        DurableApplyState::Confirmed => ApplyLedgerState::Confirmed,
        DurableApplyState::Rejected => ApplyLedgerState::Rejected,
    }
}

fn from_store_state(state: ApplyLedgerState) -> DurableApplyState {
    match state {
        ApplyLedgerState::Pending => DurableApplyState::Pending,
        ApplyLedgerState::Confirmed => DurableApplyState::Confirmed,
        ApplyLedgerState::Rejected => DurableApplyState::Rejected,
    }
}

#[async_trait]
impl DurableApplyStore for StoreDurableApplyLedger {
    async fn reserve(
        &self,
        scope: &str,
        key: &str,
        op_name: &str,
        now_secs: u64,
    ) -> Result<DurableReserve, RuntimeError> {
        let reserve = self
            .store
            .clone()
            .apply_ledger_reserve(
                scope.to_string(),
                key.to_string(),
                op_name.to_string(),
                now_secs as i64,
            )
            .await
            .map_err(store_error)?;
        Ok(match reserve {
            ApplyLedgerReserve::Reserved => DurableReserve::Reserved,
            ApplyLedgerReserve::Existing(row) => DurableReserve::Existing(DurableApplyRecord {
                op_name: row.op_name,
                state: from_store_state(row.state),
                payload_json: row.outcome_json,
            }),
        })
    }

    async fn settle(
        &self,
        scope: &str,
        key: &str,
        state: DurableApplyState,
        payload_json: &str,
        now_secs: u64,
    ) -> Result<(), RuntimeError> {
        self.store
            .clone()
            .apply_ledger_settle(
                scope.to_string(),
                key.to_string(),
                to_store_state(state),
                payload_json.to_string(),
                now_secs as i64,
            )
            .await
            .map_err(store_error)
    }

    async fn clear(&self, scope: &str, key: &str) -> Result<(), RuntimeError> {
        self.store
            .clone()
            .apply_ledger_clear(scope.to_string(), key.to_string())
            .await
            .map_err(store_error)
    }
}
