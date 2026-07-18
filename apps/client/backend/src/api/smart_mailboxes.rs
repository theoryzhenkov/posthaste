//! The smart-mailboxes family: the list with live counts, and the
//! create/update/delete/reset commands over the config-backed saved queries.

use posthaste_client_models::{
    CreateSmartMailboxIntent, DeleteSmartMailboxIntent, FieldPatch, ResetSmartMailboxesIntent,
    SmartMailboxRow, SmartMailboxesQuery, SmartMailboxesResult, UpdateSmartMailboxIntent,
};
use posthaste_domain_model::{
    validate_query, AccountId, DomainEvent, Id, MailQueryRule, MailboxRole, SmartMailbox,
    SmartMailboxId, SmartMailboxKind, EVENT_TOPIC_SMART_MAILBOX_CREATED,
    EVENT_TOPIC_SMART_MAILBOX_DELETED, EVENT_TOPIC_SMART_MAILBOX_RESET,
    EVENT_TOPIC_SMART_MAILBOX_UPDATED,
};

use super::{now_rfc3339, offload_read, ApiFailure};
use crate::AppState;

/// Account id stamped on smart-mailbox events: the configuration is
/// app-global, owned by no provider account.
const GLOBAL_EVENT_ACCOUNT_ID: &str = "app";

pub(crate) fn evaluate_smart_mailboxes(
    app: &AppState,
    _query: SmartMailboxesQuery,
) -> Result<SmartMailboxesResult, ApiFailure> {
    // The config list is already in display order (the user's explicit order,
    // then the canonical fallback) — the client renders the rows verbatim.
    let mailboxes = app.service.list_smart_mailboxes_config()?;
    let mut rows = Vec::with_capacity(mailboxes.len());
    for mailbox in mailboxes {
        let (unread_messages, total_messages) =
            app.service.count_messages_by_rule(&mailbox.rule)?;
        rows.push(SmartMailboxRow {
            id: mailbox.id,
            name: mailbox.name,
            kind: mailbox.kind,
            default_key: mailbox.default_key,
            role: mailbox.role,
            parent_id: mailbox.parent_id,
            rule: mailbox.rule,
            unread_messages,
            total_messages,
            created_at: mailbox.created_at,
            updated_at: mailbox.updated_at,
        });
    }
    Ok(SmartMailboxesResult { rows })
}

pub(crate) async fn create_smart_mailbox(
    app: &AppState,
    intent: CreateSmartMailboxIntent,
) -> Result<u64, ApiFailure> {
    let name = intent.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiFailure::malformed(
            "smart mailbox name must not be empty",
        ));
    }
    validate_rule(&intent.rule)?;
    let now = now_rfc3339();
    let smart_mailbox = SmartMailbox {
        id: SmartMailboxId::from(Id::generate()),
        name,
        kind: SmartMailboxKind::User,
        default_key: None,
        role: normalize_view_role(intent.role)?,
        parent_id: None,
        rule: intent.rule,
        created_at: now.clone(),
        updated_at: now,
    };
    // The config save is a synchronous filesystem write; offload it off the
    // async worker (matching the read-path discipline).
    let saved_id = smart_mailbox.id.clone();
    let service = app.service.clone();
    offload_read(move || Ok(service.save_smart_mailbox(&smart_mailbox)?)).await?;
    Ok(publish_smart_mailbox_event(
        app,
        EVENT_TOPIC_SMART_MAILBOX_CREATED,
        Some(&saved_id),
    ))
}

pub(crate) async fn update_smart_mailbox(
    app: &AppState,
    intent: UpdateSmartMailboxIntent,
) -> Result<u64, ApiFailure> {
    // Validate the request shape before touching the config store.
    if let Some(name) = &intent.name {
        if name.trim().is_empty() {
            return Err(ApiFailure::malformed(
                "smart mailbox name must not be empty",
            ));
        }
    }
    // `Some(None)` clears the role, `None` leaves it untouched.
    let role_update = match intent.role {
        FieldPatch::Keep => None,
        FieldPatch::Set { value } => Some(normalize_view_role(Some(value))?),
        FieldPatch::Clear => Some(None),
    };
    if let Some(rule) = &intent.rule {
        validate_rule(rule)?;
    }
    // The config read-modify-write is a synchronous filesystem round-trip;
    // offload it off the async worker (matching the read-path discipline).
    let service = app.service.clone();
    let updated_at = now_rfc3339();
    let saved_id = offload_read(move || {
        let mut smart_mailbox = service.get_smart_mailbox(&intent.smart_mailbox_id)?;
        if let Some(name) = intent.name {
            smart_mailbox.name = name.trim().to_string();
        }
        if let Some(role) = role_update {
            smart_mailbox.role = role;
        }
        if let Some(rule) = intent.rule {
            smart_mailbox.rule = rule;
        }
        smart_mailbox.updated_at = updated_at;
        service.save_smart_mailbox(&smart_mailbox)?;
        Ok(smart_mailbox.id)
    })
    .await?;
    Ok(publish_smart_mailbox_event(
        app,
        EVENT_TOPIC_SMART_MAILBOX_UPDATED,
        Some(&saved_id),
    ))
}

pub(crate) async fn delete_smart_mailbox(
    app: &AppState,
    intent: DeleteSmartMailboxIntent,
) -> Result<u64, ApiFailure> {
    // The config resolve + delete is a synchronous filesystem round-trip;
    // offload it off the async worker. Resolve the id first so an unknown
    // target is an unknown-id failure, not a phantom success (the delete is
    // otherwise silently idempotent).
    let service = app.service.clone();
    let saved_id = offload_read(move || {
        let smart_mailbox = service.get_smart_mailbox(&intent.smart_mailbox_id)?;
        service.delete_smart_mailbox(&smart_mailbox.id)?;
        Ok(smart_mailbox.id)
    })
    .await?;
    Ok(publish_smart_mailbox_event(
        app,
        EVENT_TOPIC_SMART_MAILBOX_DELETED,
        Some(&saved_id),
    ))
}

pub(crate) async fn reset_smart_mailboxes(
    app: &AppState,
    _intent: ResetSmartMailboxesIntent,
) -> Result<u64, ApiFailure> {
    // The config reset rewrites the document — a synchronous filesystem write;
    // offload it off the async worker.
    let service = app.service.clone();
    offload_read(move || Ok(service.reset_default_smart_mailboxes().map(|_| ())?)).await?;
    Ok(publish_smart_mailbox_event(
        app,
        EVENT_TOPIC_SMART_MAILBOX_RESET,
        None,
    ))
}

/// Reject a rule the store's compiler would refuse (unknown field/operator
/// pairing, malformed regex) at the write boundary, before it is saved.
fn validate_rule(rule: &MailQueryRule) -> Result<(), ApiFailure> {
    validate_query(rule)
        .map_err(|error| ApiFailure::malformed(format!("invalid smart-mailbox rule: {error}")))
}

/// Validate an optional view role: trims, treats empty as cleared, and
/// accepts only the user-assignable mailbox roles — everything the vocabulary
/// knows except the system-managed `snooze`. Returns the canonical role
/// string so storage is normalized.
fn normalize_view_role(role: Option<String>) -> Result<Option<String>, ApiFailure> {
    let Some(role) = role else {
        return Ok(None);
    };
    let trimmed = role.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match MailboxRole::parse(trimmed) {
        Some(MailboxRole::Snooze) | None => Err(ApiFailure::malformed(format!(
            "'{trimmed}' is not an assignable smart-mailbox role"
        ))),
        Some(parsed) => Ok(Some(parsed.as_str().to_string())),
    }
}

/// Publish one smart-mailbox configuration event (bumping the generation) so
/// every connected client observes the change on the stream, and return the
/// resulting generation.
fn publish_smart_mailbox_event(
    app: &AppState,
    topic: &str,
    smart_mailbox_id: Option<&SmartMailboxId>,
) -> u64 {
    app.events.publish(&[DomainEvent {
        seq: 0,
        account_id: AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
        topic: topic.to_string(),
        occurred_at: now_rfc3339(),
        mailbox_id: None,
        message_id: None,
        payload: match smart_mailbox_id {
            Some(id) => serde_json::json!({ "smartMailboxId": id.as_str() }),
            None => serde_json::Value::Null,
        },
    }]);
    app.events.generation()
}
