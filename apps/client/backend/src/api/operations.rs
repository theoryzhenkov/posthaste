//! The pending-operations family: the outbox read, plus the retry and
//! cancel commands over its rows.

use axum::http::StatusCode;
use posthaste_client_models::{
    ApiErrorKind, CancelOperationIntent, PendingOperationRow, PendingOperationsQuery,
    PendingOperationsResult, RetryOperationIntent,
};
use posthaste_domain_model::{AccountId, Operation, OperationId, OperationState};

use super::command::finish_mail_command;
use super::{offload_read, scoped_accounts, ApiFailure};
use crate::AppState;

pub(crate) fn evaluate_pending_operations(
    app: &AppState,
    query: PendingOperationsQuery,
) -> Result<PendingOperationsResult, ApiFailure> {
    let mut rows = Vec::new();
    for account_id in scoped_accounts(app, query.account_id.as_ref())? {
        for operation in app.service.list_pending_operations(&account_id)? {
            rows.push(PendingOperationRow {
                id: operation.id,
                account_id: operation.account_id,
                kind: operation.kind,
                state: operation.state,
                entity_kind: operation.entity.kind,
                entity_id: operation.entity.id,
                attempts: operation.attempts,
                last_error: operation.last_error,
                send_at: operation.send_at,
                created_at: operation.created_at,
                updated_at: operation.updated_at,
            });
        }
    }
    // Newest first across accounts; created_at is normalized RFC 3339, so
    // the lexicographic order is the chronological order.
    rows.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(PendingOperationsResult { rows })
}

pub(crate) async fn retry_operation(
    app: &AppState,
    intent: RetryOperationIntent,
) -> Result<u64, ApiFailure> {
    let operation = owned_operation(app, &intent.account_id, &intent.operation_id).await?;
    if !matches!(
        operation.state,
        OperationState::Failed | OperationState::DispatchUncertain
    ) {
        return Err(ApiFailure::new(
            StatusCode::CONFLICT,
            ApiErrorKind::Conflict,
            "only a failed or dispatch-uncertain operation can be retried",
            false,
        ));
    }
    let service = app.service.clone();
    let operation_id = intent.operation_id.clone();
    offload_read(move || Ok(service.retry_operation(&operation_id)?)).await?;
    // No events of its own: the re-armed state is pending-operations data,
    // and the bump plus the sync nudge get the flusher to it promptly.
    Ok(finish_mail_command(app, &intent.account_id, Vec::new()).await)
}

pub(crate) async fn cancel_operation(
    app: &AppState,
    intent: CancelOperationIntent,
) -> Result<u64, ApiFailure> {
    owned_operation(app, &intent.account_id, &intent.operation_id).await?;
    match app.service.discard_operation(&intent.operation_id).await? {
        // Removed: a cancelled send also unwound its folded effects and
        // restored the consumed draft, so publish everything.
        Some(events) => Ok(finish_mail_command(app, &intent.account_id, events).await),
        // Raced its settlement: the op is gone, which is queryable state.
        None => Ok(app.events.generation()),
    }
}

/// The operation, validated to exist under the intent's account.
async fn owned_operation(
    app: &AppState,
    account_id: &AccountId,
    operation_id: &OperationId,
) -> Result<Operation, ApiFailure> {
    let service = app.service.clone();
    let owned_id = operation_id.clone();
    let operation = offload_read(move || Ok(service.get_operation(&owned_id)?)).await?;
    operation
        .filter(|operation| operation.account_id == *account_id)
        .ok_or_else(|| ApiFailure::unknown_id(format!("operation {}", operation_id.as_str())))
}
