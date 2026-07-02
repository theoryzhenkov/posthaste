use posthaste_domain_service::{
    now_iso8601 as domain_now_iso8601, AccountId, GatewayError, PushNotification,
};

/// Convert a raw JMAP WebSocket push object into a domain notification.
///
/// Only state-change push objects are relevant to sync triggers. Other push
/// object variants are ignored.
///
/// @spec docs/L1-jmap#push
pub(crate) fn convert_ws_push_object(
    account_id: &AccountId,
    server_account_id: &str,
    push: jmap_client::PushObject,
) -> Result<Option<PushNotification>, GatewayError> {
    match push {
        jmap_client::PushObject::StateChange { mut changed } => {
            state_changes_to_notification(account_id, changed.remove(server_account_id), None)
        }
        _ => Ok(None),
    }
}

/// Convert a raw JMAP SSE push notification into a domain notification.
///
/// Calendar alerts are not sync state changes and are ignored.
///
/// @spec docs/L1-jmap#push
pub(crate) fn convert_sse_push_notification(
    account_id: &AccountId,
    server_account_id: &str,
    push: jmap_client::event_source::PushNotification,
) -> Result<Option<PushNotification>, GatewayError> {
    match push {
        jmap_client::event_source::PushNotification::StateChange(mut changes) => {
            let checkpoint = changes.id().map(str::to_string);
            state_changes_to_notification(
                account_id,
                changes.account_changes(server_account_id),
                checkpoint,
            )
        }
        jmap_client::event_source::PushNotification::CalendarAlert(_) => Ok(None),
    }
}

fn state_changes_to_notification<I>(
    account_id: &AccountId,
    changed_entries: Option<I>,
    checkpoint: Option<String>,
) -> Result<Option<PushNotification>, GatewayError>
where
    I: IntoIterator<Item = (jmap_client::DataType, String)>,
{
    let changed: Vec<String> = changed_entries
        .map(|entries| {
            entries
                .into_iter()
                .map(|(data_type, _)| data_type.to_string())
                .collect()
        })
        .unwrap_or_default();
    if changed.is_empty() && checkpoint.is_none() {
        return Ok(None);
    }
    let received_at = domain_now_iso8601().map_err(GatewayError::Rejected)?;

    Ok(Some(PushNotification {
        account_id: account_id.clone(),
        changed,
        received_at,
        checkpoint,
    }))
}

#[cfg(test)]
mod tests;
