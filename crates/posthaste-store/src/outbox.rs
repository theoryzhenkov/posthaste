//! Durable Tier-2 (runtime <-> provider) command outbox persistence.
//!
//! @spec docs/L1-outbox#operation-model

use super::*;

/// Bound on [`OperationOutboxStore::list_flushable_operations`]: the flush
/// loop (`flush_account` in `posthaste-domain-service`) already re-invokes
/// this once per sync cycle, and each op it processes transitions out of the
/// flushable state set before the next call — so a bounded batch per call
/// drains a large/stuck outbox across cycles instead of materializing it all
/// into one unbounded `Vec` in a single query (N15 / RFC-L2-lifecycle D67(b)
/// / M27 sub-unit (b)). Sized to comfortably clear within one
/// `ARM_BUDGET_SYNC` cycle under normal per-op provider latency, not to
/// guarantee full drainage of a pathological backlog in one pass — the next
/// cycle picks up whatever remains. **Review** (picked sane, not measured).
pub(crate) const OUTBOX_FLUSH_BATCH_LIMIT: i64 = 200;

/// Bound on `list_pending_operations`/`list_unsettled_operations`. Unlike
/// `list_flushable_operations` these two are not drained in a retry loop —
/// `list_pending_operations` backs a compose/detail UI listing and
/// `list_unsettled_operations` is folded over to compute a message's
/// remaining state assertions at settlement time — so an aggressive per-cycle
/// batch would risk silently truncating a correctness-relevant read instead
/// of just being retried later. This is a generous worst-case safety cap
/// only: it bounds the pathological case (N15) without changing behavior for
/// any realistic per-account backlog. **Review**.
pub(crate) const OUTBOX_LIST_SAFETY_LIMIT: i64 = 5_000;

fn parse_operation_state(value: &str) -> Result<OperationState, StoreError> {
    match value {
        "pending" => Ok(OperationState::Pending),
        "inflight" => Ok(OperationState::Inflight),
        "applied" => Ok(OperationState::Applied),
        // Legacy dogfood rows from the first outbox design parked forever as
        // `conflicted`; recover them into the new retryable state so they can
        // drain under the assertion-based flush model.
        "conflicted" => Ok(OperationState::Pending),
        "failed" => Ok(OperationState::Failed),
        "dispatchUncertain" => Ok(OperationState::DispatchUncertain),
        other => Err(StoreError::Failure(format!(
            "unknown outbox operation state: {other}"
        ))),
    }
}

fn operation_state_str(state: OperationState) -> &'static str {
    match state {
        OperationState::Pending => "pending",
        OperationState::Inflight => "inflight",
        OperationState::Applied => "applied",
        OperationState::Failed => "failed",
        // Parked sends are excluded from `list_flushable_operations` by omission
        // from its state set — a possibly-delivered send is never auto-resent.
        OperationState::DispatchUncertain => "dispatchUncertain",
    }
}

fn parse_operation_kind(value: &str) -> Result<OperationKind, StoreError> {
    match value {
        "setKeywords" => Ok(OperationKind::SetKeywords),
        "replaceMailboxes" => Ok(OperationKind::ReplaceMailboxes),
        "destroy" => Ok(OperationKind::Destroy),
        "draftCreate" => Ok(OperationKind::DraftCreate),
        "draftUpdate" => Ok(OperationKind::DraftUpdate),
        "draftDelete" => Ok(OperationKind::DraftDelete),
        "send" => Ok(OperationKind::Send),
        other => Err(StoreError::Failure(format!(
            "unknown outbox operation kind: {other}"
        ))),
    }
}

fn operation_kind_str(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::SetKeywords => "setKeywords",
        OperationKind::ReplaceMailboxes => "replaceMailboxes",
        OperationKind::Destroy => "destroy",
        OperationKind::DraftCreate => "draftCreate",
        OperationKind::DraftUpdate => "draftUpdate",
        OperationKind::DraftDelete => "draftDelete",
        OperationKind::Send => "send",
    }
}

fn parse_entity_kind(value: &str) -> Result<OperationEntityKind, StoreError> {
    match value {
        "message" => Ok(OperationEntityKind::Message),
        "draft" => Ok(OperationEntityKind::Draft),
        other => Err(StoreError::Failure(format!(
            "unknown outbox entity kind: {other}"
        ))),
    }
}

fn entity_kind_str(kind: OperationEntityKind) -> &'static str {
    match kind {
        OperationEntityKind::Message => "message",
        OperationEntityKind::Draft => "draft",
    }
}

/// Columns selected by every operation read, in struct order.
const OPERATION_COLUMNS: &str = "id, account_id, entity_kind, entity_id, kind, payload, \
     state, attempts, last_error, depends_on, created_at, updated_at";

fn row_to_operation(row: &Row) -> rusqlite::Result<Result<Operation, StoreError>> {
    // Extract every column first so all `rusqlite::Error`s surface through the
    // outer result; the inner closure then only does `StoreError` parsing.
    let id: String = row.get(0)?;
    let account_id: String = row.get(1)?;
    let entity_kind_str: String = row.get(2)?;
    let entity_id: String = row.get(3)?;
    let kind_str: String = row.get(4)?;
    let payload_str: String = row.get(5)?;
    let state_str: String = row.get(6)?;
    let attempts: i64 = row.get(7)?;
    let last_error: Option<String> = row.get(8)?;
    let depends_on: Option<String> = row.get(9)?;
    let created_at: String = row.get(10)?;
    let updated_at: String = row.get(11)?;
    Ok((|| {
        let payload: Value = serde_json::from_str(&payload_str)
            .map_err(|error| StoreError::Failure(format!("invalid outbox payload: {error}")))?;
        Ok(Operation {
            id: OperationId::from(id),
            account_id: AccountId::from(account_id),
            entity: OperationEntity {
                kind: parse_entity_kind(&entity_kind_str)?,
                id: entity_id,
            },
            kind: parse_operation_kind(&kind_str)?,
            payload,
            state: parse_operation_state(&state_str)?,
            attempts: attempts.max(0) as u32,
            last_error,
            depends_on: depends_on.map(OperationId::from),
            created_at,
            updated_at,
        })
    })())
}

fn collect_operations(
    connection: &Connection,
    sql: &str,
    account_id: &AccountId,
    limit: i64,
) -> Result<Vec<Operation>, StoreError> {
    let mut statement = connection.prepare(sql).map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), limit], row_to_operation)
        .map_err(sql_to_store_error)?;
    let mut operations = Vec::new();
    for row in rows {
        operations.push(row.map_err(sql_to_store_error)??);
    }
    Ok(operations)
}

impl OperationOutboxStore for DatabaseStore {
    fn enqueue_operation(&self, operation: &Operation) -> Result<Operation, StoreError> {
        let payload = serde_json::to_string(&operation.payload)
            .map_err(|error| StoreError::Failure(format!("invalid outbox payload: {error}")))?;
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO outbox_operation (
                    id, account_id, entity_kind, entity_id, kind, payload,
                    state, attempts, last_error, depends_on, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    operation.id.as_str(),
                    operation.account_id.as_str(),
                    entity_kind_str(operation.entity.kind),
                    operation.entity.id,
                    operation_kind_str(operation.kind),
                    payload,
                    operation_state_str(operation.state),
                    operation.attempts as i64,
                    operation.last_error,
                    operation.depends_on.as_ref().map(OperationId::as_str),
                    operation.created_at,
                    operation.updated_at,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })?;
        self.get_operation(&operation.id)?
            .ok_or_else(|| StoreError::Failure("outbox operation was not persisted".to_string()))
    }

    fn list_flushable_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError> {
        let connection = self.read_connection()?;
        collect_operations(
            &connection,
            &format!(
                "SELECT {OPERATION_COLUMNS} FROM outbox_operation
                 WHERE account_id = ?1 AND state IN ('pending', 'inflight', 'conflicted')
                 ORDER BY rowid ASC
                 LIMIT ?2"
            ),
            account_id,
            OUTBOX_FLUSH_BATCH_LIMIT,
        )
    }

    fn list_pending_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError> {
        let connection = self.read_connection()?;
        collect_operations(
            &connection,
            &format!(
                "SELECT {OPERATION_COLUMNS} FROM outbox_operation
                 WHERE account_id = ?1 AND state != 'applied'
                 ORDER BY rowid ASC
                 LIMIT ?2"
            ),
            account_id,
            OUTBOX_LIST_SAFETY_LIMIT,
        )
    }

    fn list_unsettled_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError> {
        let connection = self.read_connection()?;
        collect_operations(
            &connection,
            &format!(
                "SELECT {OPERATION_COLUMNS} FROM outbox_operation
                 WHERE account_id = ?1 AND state != 'failed'
                 ORDER BY rowid ASC
                 LIMIT ?2"
            ),
            account_id,
            OUTBOX_LIST_SAFETY_LIMIT,
        )
    }

    fn get_operation(&self, id: &OperationId) -> Result<Option<Operation>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {OPERATION_COLUMNS} FROM outbox_operation WHERE id = ?1"
            ))
            .map_err(sql_to_store_error)?;
        statement
            .query_row(params![id.as_str()], row_to_operation)
            .optional()
            .map_err(sql_to_store_error)?
            .transpose()
    }

    fn update_operation_state(
        &self,
        id: &OperationId,
        state: OperationState,
        attempts: u32,
        last_error: Option<&str>,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE outbox_operation
                 SET state = ?2, attempts = ?3, last_error = ?4, updated_at = ?5
                 WHERE id = ?1",
                params![
                    id.as_str(),
                    operation_state_str(state),
                    attempts as i64,
                    last_error,
                    now_iso8601()?,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn reconcile_operation_entity_id(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE outbox_operation
                 SET entity_id = ?3, updated_at = ?4
                 WHERE account_id = ?1 AND entity_id = ?2",
                params![
                    account_id.as_str(),
                    from_entity_id,
                    to_entity_id,
                    now_iso8601()?,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn remove_operation(&self, id: &OperationId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM outbox_operation WHERE id = ?1",
                params![id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

/// M68/M69: the draft-identity methods behind the `DraftRegistry` port, backed
/// by the `draft_alias` table (the schema rename to `draft_registry` is M73).
/// Since M69 the table is the SINGLE authority for the stable-key → live-entity
/// mapping: sync writes through to it in the same transaction as every message
/// upsert/prune (`mutations/sync_batch.rs`), so resolution is one SELECT — the
/// D131 alias-then-projection fallback is gone.
impl DraftRegistry for DatabaseStore {
    fn resolve_draft_entity(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<Option<String>, StoreError> {
        let connection = self.read_connection()?;
        // M69 (D135): ONE authority, ONE lookup. The registry is fresh in every
        // regime — this runtime's save/rotate paths write it at enqueue/flush,
        // and sync writes through in the same transaction as the projection —
        // so there is no fallback and no precedence to arbitrate.
        let mut statement = connection
            .prepare("SELECT entity_id FROM draft_alias WHERE account_id = ?1 AND draft_key = ?2")
            .map_err(sql_to_store_error)?;
        statement
            .query_row(params![account_id.as_str(), draft_key], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(sql_to_store_error)
    }

    fn set_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
        entity_id: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO draft_alias (account_id, draft_key, entity_id)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, draft_key) DO UPDATE SET entity_id = excluded.entity_id",
                params![account_id.as_str(), draft_key, entity_id],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn update_draft_alias_entity(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE draft_alias SET entity_id = ?3
                 WHERE account_id = ?1 AND entity_id = ?2",
                params![account_id.as_str(), from_entity_id, to_entity_id],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn remove_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM draft_alias WHERE account_id = ?1 AND draft_key = ?2",
                params![account_id.as_str(), draft_key],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}
