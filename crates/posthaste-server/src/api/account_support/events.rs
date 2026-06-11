use super::*;

pub(crate) const GLOBAL_EVENT_ACCOUNT_ID: &str = "app";

/// Typed resource kind values serialized in domain event `resources[]` payloads.
///
/// @spec docs/L1-sync#event-propagation
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResourceKind {
    Account,
    AppSettings,
    Config,
    SmartMailbox,
}

/// Typed resource operation values serialized in domain event `resources[]` payloads.
///
/// @spec docs/L1-sync#event-propagation
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ResourceOperation {
    Created,
    Updated,
    Deleted,
    Reset,
    Reloaded,
}

/// Declarative resource-change payload item for domain events.
///
/// The serialized shape is intentionally stable:
/// `{ kind, operation, id?, accountId? }`.
///
/// @spec docs/L1-sync#event-propagation
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

impl ResourceChange {
    pub(crate) fn account(operation: ResourceOperation, account_id: &AccountId) -> Self {
        Self {
            kind: ResourceKind::Account,
            operation,
            id: Some(account_id.as_str().to_string()),
            account_id: Some(account_id.as_str().to_string()),
        }
    }

    pub(crate) fn app_settings_updated() -> Self {
        Self {
            kind: ResourceKind::AppSettings,
            operation: ResourceOperation::Updated,
            id: None,
            account_id: None,
        }
    }

    pub(crate) fn config_reloaded() -> Self {
        Self {
            kind: ResourceKind::Config,
            operation: ResourceOperation::Reloaded,
            id: None,
            account_id: None,
        }
    }

    pub(crate) fn smart_mailbox(
        operation: ResourceOperation,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Self {
        Self {
            kind: ResourceKind::SmartMailbox,
            operation,
            id: Some(smart_mailbox_id.as_str().to_string()),
            account_id: None,
        }
    }

    pub(crate) fn smart_mailbox_reset() -> Self {
        Self {
            kind: ResourceKind::SmartMailbox,
            operation: ResourceOperation::Reset,
            id: None,
            account_id: None,
        }
    }
}

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

/// Append a global config/resource event to the event log and broadcast it.
///
/// @spec docs/L1-sync#event-propagation
pub(crate) fn append_and_publish_config_event(
    state: &Arc<AppState>,
    topic: &str,
    resources: Vec<ResourceChange>,
    extra: serde_json::Value,
) -> Result<(), posthaste_domain::StoreError> {
    let mut payload = match extra {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    payload.insert("resources".to_string(), json!(resources));
    let event = state.store.append_event(
        &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
        topic,
        None,
        None,
        serde_json::Value::Object(payload),
    )?;
    state.publish_events(&[event]);
    Ok(())
}

/// Convert a store-level error into an API error.
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

/// Generate a smart mailbox ID from a human name: `sm-{slug}-{uuid}`.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub(crate) fn generate_smart_mailbox_id(name: &str) -> String {
    let slug = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!(
        "sm-{}-{}",
        if slug.is_empty() {
            "mailbox"
        } else {
            slug.as_str()
        },
        Uuid::new_v4()
    )
}

/// Generate an internal account ID from identity fields. The ID is deliberately
/// hidden from the UI; it only needs to be stable after account creation.
pub(crate) fn generate_account_id_seed(name: &str, email_patterns: &[String]) -> String {
    let seed = email_patterns
        .iter()
        .map(|pattern| pattern.trim())
        .find(|pattern| !pattern.is_empty())
        .unwrap_or_else(|| name.trim());
    let slug = seed
        .trim_start_matches("*@")
        .trim_start_matches('@')
        .to_lowercase()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        "account".to_string()
    } else {
        slug
    }
}
