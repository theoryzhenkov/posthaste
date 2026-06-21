//! Durable Tier-2 (runtime <-> provider) command outbox persistence.
//!
//! @spec docs/L1-outbox#operation-model

use super::*;

fn parse_operation_state(value: &str) -> Result<OperationState, StoreError> {
    match value {
        "pending" => Ok(OperationState::Pending),
        "inflight" => Ok(OperationState::Inflight),
        "applied" => Ok(OperationState::Applied),
        "conflicted" => Ok(OperationState::Conflicted),
        "failed" => Ok(OperationState::Failed),
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
        OperationState::Conflicted => "conflicted",
        OperationState::Failed => "failed",
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
     base_cursor, state, attempts, last_error, depends_on, created_at, updated_at";

fn row_to_operation(row: &Row) -> rusqlite::Result<Result<Operation, StoreError>> {
    // Extract every column first so all `rusqlite::Error`s surface through the
    // outer result; the inner closure then only does `StoreError` parsing.
    let id: String = row.get(0)?;
    let account_id: String = row.get(1)?;
    let entity_kind_str: String = row.get(2)?;
    let entity_id: String = row.get(3)?;
    let kind_str: String = row.get(4)?;
    let payload_str: String = row.get(5)?;
    let base_cursor: Option<String> = row.get(6)?;
    let state_str: String = row.get(7)?;
    let attempts: i64 = row.get(8)?;
    let last_error: Option<String> = row.get(9)?;
    let depends_on: Option<String> = row.get(10)?;
    let created_at: String = row.get(11)?;
    let updated_at: String = row.get(12)?;
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
            base_cursor,
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
) -> Result<Vec<Operation>, StoreError> {
    let mut statement = connection.prepare(sql).map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str()], row_to_operation)
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
                    id, account_id, entity_kind, entity_id, kind, payload, base_cursor,
                    state, attempts, last_error, depends_on, created_at, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    operation.id.as_str(),
                    operation.account_id.as_str(),
                    entity_kind_str(operation.entity.kind),
                    operation.entity.id,
                    operation_kind_str(operation.kind),
                    payload,
                    operation.base_cursor,
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
                 WHERE account_id = ?1 AND state IN ('pending', 'conflicted')
                 ORDER BY rowid ASC"
            ),
            account_id,
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
                 ORDER BY rowid ASC"
            ),
            account_id,
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
