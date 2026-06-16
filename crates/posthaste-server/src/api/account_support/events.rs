use super::*;

/// Typed resource kind values serialized in domain event `resources[]` payloads.
///
/// @spec docs/L1-sync#event-propagation
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResourceKind {
    Account,
}

/// Typed resource operation values serialized in domain event `resources[]` payloads.
///
/// @spec docs/L1-sync#event-propagation
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResourceOperation {
    Created,
    Updated,
    Deleted,
}

/// Declarative resource-change payload item for domain events.
///
/// The serialized shape is intentionally stable:
/// `{ kind, operation, id?, accountId? }`.
///
/// @spec docs/L1-sync#event-propagation
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceChange {
    kind: ResourceKind,
    operation: ResourceOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}

#[cfg(test)]
impl ResourceChange {
    pub(crate) fn account(operation: ResourceOperation, account_id: &AccountId) -> Self {
        Self {
            kind: ResourceKind::Account,
            operation,
            id: Some(account_id.as_str().to_string()),
            account_id: Some(account_id.as_str().to_string()),
        }
    }
}

#[cfg(test)]
fn account_operation_from_topic(topic: &str) -> ResourceOperation {
    match topic {
        EVENT_TOPIC_ACCOUNT_CREATED => ResourceOperation::Created,
        EVENT_TOPIC_ACCOUNT_DELETED => ResourceOperation::Deleted,
        _ => ResourceOperation::Updated,
    }
}

/// Append an account lifecycle event to the event log and broadcast it.
///
/// @spec docs/L1-sync#event-propagation
#[cfg(test)]
pub(crate) fn append_and_publish_account_event(
    state: &Arc<AppState>,
    account_id: &AccountId,
    topic: &str,
) -> Result<(), posthaste_domain::StoreError> {
    let operation = account_operation_from_topic(topic);
    let event = state.store.append_event(
        account_id,
        topic,
        None,
        None,
        json!({
            "accountId": account_id.as_str(),
            "resources": [ResourceChange::account(operation, account_id)],
        }),
    )?;
    state.publish_events(&[event]);
    Ok(())
}

/// Convert a store-level error into an API error.
#[cfg(test)]
pub(crate) fn store_error_to_api(error: posthaste_domain::StoreError) -> ApiError {
    ApiError::from_service_error(ServiceError::from(error))
}

/// Construct a 500 Internal Server Error from a message string.
pub(crate) fn internal_error(error: String) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiErrorCode::InternalError,
        error,
    )
}
