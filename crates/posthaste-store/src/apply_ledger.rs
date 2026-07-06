//! Durable direct-apply idempotency ledger persistence (DS7 — mail-safety
//! durability).
//!
//! The runtime's in-memory apply ledger (`posthaste-runtime` `apply_ledger.rs`)
//! is TTL/cap-bounded and lost on restart, so a redelivery arriving after the
//! TTL reap or after a process restart would re-execute an already-applied
//! keyed operation (a possible double-send). This table is the DURABLE source
//! of truth for "already applied": the keyed decision is reserved `pending`
//! BEFORE the operation executes and settled with the outcome after, so a
//! redelivery always finds either the prior outcome (re-observed, never
//! re-executed) or the unresolved `pending` crash marker (conservatively NOT
//! re-executed — the outcome is unknown).
//!
//! Retention: settled rows are GC'd opportunistically on each reservation, but
//! only past [`APPLY_LEDGER_RETENTION_SECS`] — a horizon that dominates any
//! realistic redelivery window (webhook/agent retry horizons are hours to a
//! few days; the in-memory ledger's TTL is 15 minutes). `pending` rows are
//! never GC'd: an unresolved crash marker must keep blocking re-execution
//! until an operator (or a future reconciliation) resolves it.

use std::sync::Arc;

use super::*;

/// How long a SETTLED apply decision is retained, in seconds: 30 days. Must
/// dominate every realistic redelivery window (the in-memory ledger's 15-minute
/// TTL, webhook redelivery horizons of hours–days, and a deliberate client
/// retry after an outage) so a redelivery can never miss the recorded decision.
/// `pending` rows are exempt (never GC'd).
pub const APPLY_LEDGER_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;

/// The durable state of an apply-ledger row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyLedgerState {
    /// Reserved before execution; the outcome was never recorded (in flight,
    /// or a crash between apply and record). Never GC'd.
    Pending,
    /// The operation applied; `outcome_json` carries the stored outcome.
    Confirmed,
    /// The operation was permanently rejected; `outcome_json` carries the
    /// rejection envelope.
    Rejected,
}

impl ApplyLedgerState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirmed => "confirmed",
            Self::Rejected => "rejected",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "confirmed" => Ok(Self::Confirmed),
            "rejected" => Ok(Self::Rejected),
            other => Err(StoreError::Failure(format!(
                "unknown apply_ledger state: {other}"
            ))),
        }
    }
}

/// A stored apply decision, as read back on a duplicate reservation.
#[derive(Clone, Debug)]
pub struct ApplyLedgerRow {
    pub op_name: String,
    pub state: ApplyLedgerState,
    pub outcome_json: Option<String>,
}

/// The verdict of [`DatabaseStore::apply_ledger_reserve`].
#[derive(Debug)]
pub enum ApplyLedgerReserve {
    /// First durable sight of this `(scope, key)`: a `pending` row was
    /// inserted; the caller executes and then settles (or clears).
    Reserved,
    /// The key already has a durable record — a prior decision (or an
    /// unresolved `pending`) the caller must honor without executing.
    Existing(ApplyLedgerRow),
}

impl DatabaseStore {
    /// Atomically look up `(scope, key)` and, when absent, insert a `pending`
    /// reservation — one write transaction, so apply-without-recording is
    /// impossible: the durable marker exists BEFORE the operation executes.
    /// Opportunistically GCs settled rows past [`APPLY_LEDGER_RETENTION_SECS`]
    /// in the same transaction (`pending` rows are never GC'd).
    pub async fn apply_ledger_reserve(
        self: Arc<Self>,
        scope: String,
        key: String,
        op_name: String,
        now_secs: i64,
    ) -> Result<ApplyLedgerReserve, StoreError> {
        self.write_transaction_async(move |tx| {
            // Opportunistic retention GC — settled rows only, past the horizon.
            tx.execute(
                "DELETE FROM apply_ledger
                  WHERE settled_at IS NOT NULL AND settled_at < ?1",
                params![now_secs.saturating_sub(APPLY_LEDGER_RETENTION_SECS)],
            )
            .map_err(sql_to_store_error)?;

            let existing = tx
                .query_row(
                    "SELECT op_name, state, outcome_json FROM apply_ledger
                      WHERE scope = ?1 AND idempotency_key = ?2",
                    params![scope, key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(sql_to_store_error)?;
            if let Some((op_name, state, outcome_json)) = existing {
                return Ok(ApplyLedgerReserve::Existing(ApplyLedgerRow {
                    op_name,
                    state: ApplyLedgerState::parse(&state)?,
                    outcome_json,
                }));
            }
            tx.execute(
                "INSERT INTO apply_ledger
                     (scope, idempotency_key, op_name, state, outcome_json,
                      created_at, settled_at)
                 VALUES (?1, ?2, ?3, 'pending', NULL, ?4, NULL)",
                params![scope, key, op_name, now_secs],
            )
            .map_err(sql_to_store_error)?;
            Ok(ApplyLedgerReserve::Reserved)
        })
        .await
    }

    /// Record the terminal decision for a reserved `(scope, key)`:
    /// `Confirmed` with the outcome payload or `Rejected` with the rejection
    /// envelope. A missing row is a no-op (already cleared).
    pub async fn apply_ledger_settle(
        self: Arc<Self>,
        scope: String,
        key: String,
        state: ApplyLedgerState,
        outcome_json: String,
        now_secs: i64,
    ) -> Result<(), StoreError> {
        self.write_transaction_async(move |tx| {
            tx.execute(
                "UPDATE apply_ledger
                    SET state = ?3, outcome_json = ?4, settled_at = ?5
                  WHERE scope = ?1 AND idempotency_key = ?2",
                params![scope, key, state.as_str(), outcome_json, now_secs],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
        .await
    }

    /// Drop the durable record for `(scope, key)` — a transient failure, so a
    /// deliberate retry re-reserves and re-executes (D47's `Failed` rule).
    pub async fn apply_ledger_clear(
        self: Arc<Self>,
        scope: String,
        key: String,
    ) -> Result<(), StoreError> {
        self.write_transaction_async(move |tx| {
            tx.execute(
                "DELETE FROM apply_ledger
                  WHERE scope = ?1 AND idempotency_key = ?2",
                params![scope, key],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_store(dir: &TempDir) -> Arc<DatabaseStore> {
        Arc::new(
            DatabaseStore::open(dir.path().join("mail.db"), dir.path().join("data"))
                .expect("open store"),
        )
    }

    // The DS7 core property at the store level: a decision settled by one store
    // instance is found by a FRESH instance over the same database file — the
    // decision survives a process restart.
    #[tokio::test]
    async fn settled_decision_survives_reopen() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir);
        let reserve = store
            .clone()
            .apply_ledger_reserve(
                "src:Api".into(),
                "send-1".into(),
                "message.send".into(),
                100,
            )
            .await
            .unwrap();
        assert!(matches!(reserve, ApplyLedgerReserve::Reserved));
        store
            .clone()
            .apply_ledger_settle(
                "src:Api".into(),
                "send-1".into(),
                ApplyLedgerState::Confirmed,
                "{\"ack\":{\"events\":[]}}".into(),
                101,
            )
            .await
            .unwrap();
        drop(store);

        // A fresh store over the same file (the restart) finds the decision.
        let reopened = open_store(&dir);
        match reopened
            .apply_ledger_reserve(
                "src:Api".into(),
                "send-1".into(),
                "message.send".into(),
                200,
            )
            .await
            .unwrap()
        {
            ApplyLedgerReserve::Existing(row) => {
                assert_eq!(row.op_name, "message.send");
                assert_eq!(row.state, ApplyLedgerState::Confirmed);
                assert_eq!(
                    row.outcome_json.as_deref(),
                    Some("{\"ack\":{\"events\":[]}}")
                );
            }
            ApplyLedgerReserve::Reserved => {
                panic!("a settled decision must survive reopen, not re-reserve")
            }
        }
    }

    // An unsettled reservation (crash between apply and record) is re-observed
    // as `pending` — never silently re-reserved — so the caller can refuse to
    // re-execute an operation whose outcome is unknown.
    #[tokio::test]
    async fn unsettled_pending_is_re_observed_not_re_reserved() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir);
        store
            .clone()
            .apply_ledger_reserve(
                "src:Api".into(),
                "send-1".into(),
                "message.send".into(),
                100,
            )
            .await
            .unwrap();
        drop(store);

        let reopened = open_store(&dir);
        match reopened
            .apply_ledger_reserve(
                "src:Api".into(),
                "send-1".into(),
                "message.send".into(),
                200,
            )
            .await
            .unwrap()
        {
            ApplyLedgerReserve::Existing(row) => {
                assert_eq!(row.state, ApplyLedgerState::Pending);
                assert!(row.outcome_json.is_none());
            }
            ApplyLedgerReserve::Reserved => {
                panic!("an unresolved pending must be re-observed, not re-reserved")
            }
        }
    }

    // Clearing (a transient failure) frees the key for a deliberate retry.
    #[tokio::test]
    async fn clear_frees_the_key_for_a_retry() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir);
        store
            .clone()
            .apply_ledger_reserve(
                "src:Api".into(),
                "op-1".into(),
                "message.destroy".into(),
                100,
            )
            .await
            .unwrap();
        store
            .clone()
            .apply_ledger_clear("src:Api".into(), "op-1".into())
            .await
            .unwrap();
        assert!(matches!(
            store
                .apply_ledger_reserve(
                    "src:Api".into(),
                    "op-1".into(),
                    "message.destroy".into(),
                    101
                )
                .await
                .unwrap(),
            ApplyLedgerReserve::Reserved
        ));
    }

    // Retention: a settled row past APPLY_LEDGER_RETENTION_SECS is GC'd on the
    // next reservation; within the horizon it survives. A pending row is NEVER
    // GC'd regardless of age.
    #[tokio::test]
    async fn retention_gc_reaps_only_settled_rows_past_the_horizon() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir);
        // A settled row at t=100 and a pending row at t=100.
        store
            .clone()
            .apply_ledger_reserve("s".into(), "settled".into(), "message.send".into(), 100)
            .await
            .unwrap();
        store
            .clone()
            .apply_ledger_settle(
                "s".into(),
                "settled".into(),
                ApplyLedgerState::Confirmed,
                "{}".into(),
                100,
            )
            .await
            .unwrap();
        store
            .clone()
            .apply_ledger_reserve("s".into(), "crashed".into(), "message.send".into(), 100)
            .await
            .unwrap();

        // Within the horizon: the settled decision is still found.
        let within = 100 + APPLY_LEDGER_RETENTION_SECS - 1;
        assert!(matches!(
            store
                .clone()
                .apply_ledger_reserve("s".into(), "settled".into(), "message.send".into(), within)
                .await
                .unwrap(),
            ApplyLedgerReserve::Existing(_)
        ));

        // Past the horizon: the settled row is reaped (a fresh reservation),
        // but the pending crash marker still blocks.
        let past = 100 + APPLY_LEDGER_RETENTION_SECS + 1;
        assert!(matches!(
            store
                .clone()
                .apply_ledger_reserve("s".into(), "settled".into(), "message.send".into(), past)
                .await
                .unwrap(),
            ApplyLedgerReserve::Reserved
        ));
        match store
            .apply_ledger_reserve("s".into(), "crashed".into(), "message.send".into(), past)
            .await
            .unwrap()
        {
            ApplyLedgerReserve::Existing(row) => assert_eq!(row.state, ApplyLedgerState::Pending),
            ApplyLedgerReserve::Reserved => panic!("a pending crash marker must never be GC'd"),
        }
    }

    // Scopes are independent buckets: the same key under two scopes is two
    // reservations (mirrors the in-memory ledger's per-scope ledgers).
    #[tokio::test]
    async fn scopes_are_independent() {
        let dir = TempDir::new().unwrap();
        let store = open_store(&dir);
        assert!(matches!(
            store
                .clone()
                .apply_ledger_reserve("a".into(), "k".into(), "message.send".into(), 1)
                .await
                .unwrap(),
            ApplyLedgerReserve::Reserved
        ));
        assert!(matches!(
            store
                .apply_ledger_reserve("b".into(), "k".into(), "message.send".into(), 1)
                .await
                .unwrap(),
            ApplyLedgerReserve::Reserved
        ));
    }
}
