use posthaste_domain::{
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
mod tests {
    use posthaste_domain::AccountId;
    use serde_json::json;

    use super::{convert_sse_push_notification, convert_ws_push_object};

    #[test]
    fn ws_push_filters_by_server_account_and_emits_local_account() {
        let push = serde_json::from_value(json!({
            "@type": "StateChange",
            "changed": {
                "server-account": {
                    "Email": "state-1",
                    "Mailbox": "state-2"
                }
            }
        }))
        .expect("push object");

        let notification =
            convert_ws_push_object(&AccountId::from("local-account"), "server-account", push)
                .expect("conversion")
                .expect("notification");

        assert_eq!(notification.account_id, AccountId::from("local-account"));
        assert_eq!(notification.checkpoint, None);
        assert_eq!(notification.changed.len(), 2);
        assert!(notification.changed.contains(&"Email".to_string()));
        assert!(notification.changed.contains(&"Mailbox".to_string()));
    }

    #[test]
    fn ws_push_ignores_other_server_accounts() {
        let push = serde_json::from_value(json!({
            "@type": "StateChange",
            "changed": {
                "other-server-account": {
                    "Email": "state-1"
                }
            }
        }))
        .expect("push object");

        let notification =
            convert_ws_push_object(&AccountId::from("local-account"), "server-account", push)
                .expect("conversion");

        assert!(notification.is_none());
    }

    #[test]
    fn sse_push_filters_by_server_account_and_preserves_checkpoint() {
        let push = jmap_client::event_source::PushNotification::StateChange(
            serde_json::from_value(changes()).expect("changes"),
        );

        let notification = convert_sse_push_notification(
            &AccountId::from("local-account"),
            "server-account",
            push,
        )
        .expect("conversion")
        .expect("notification");

        assert_eq!(notification.account_id, AccountId::from("local-account"));
        assert_eq!(notification.checkpoint, Some("event-42".to_string()));
        assert_eq!(notification.changed.len(), 2);
        assert!(notification.changed.contains(&"Email".to_string()));
        assert!(notification.changed.contains(&"Mailbox".to_string()));
    }

    #[test]
    fn sse_push_ignores_other_server_accounts() {
        let push = jmap_client::event_source::PushNotification::StateChange(
            serde_json::from_value(changes()).expect("changes"),
        );

        let notification = convert_sse_push_notification(
            &AccountId::from("local-account"),
            "missing-account",
            push,
        )
        .expect("conversion");

        let notification = notification.expect("checkpoint-only notification");
        assert_eq!(notification.account_id, AccountId::from("local-account"));
        assert_eq!(notification.checkpoint, Some("event-42".to_string()));
        assert!(notification.changed.is_empty());
    }

    fn changes() -> serde_json::Value {
        json!({
            "id": "event-42",
            "changes": {
                "server-account": {
                    "Email": "state-1",
                    "Mailbox": "state-2"
                }
            }
        })
    }
}
