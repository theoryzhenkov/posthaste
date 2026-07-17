//! The snooze family: park a message in the snooze-role mailbox with a
//! return time, or bring it back to the inbox now. The due-time auto-return
//! runs server-side (the supervisor's scheduler), so a snoozed message comes
//! back without any client involvement.

use axum::http::StatusCode;
use posthaste_client_models::{ApiErrorKind, SnoozeIntent, UnsnoozeIntent};
use posthaste_domain_model::{AccountId, MailboxId, MailboxRole, ReplaceMailboxesCommand};

use super::command::finish_mail_command;
use super::{offload_read, scoped_accounts, ApiFailure};
use crate::AppState;

pub(crate) async fn snooze(app: &AppState, intent: SnoozeIntent) -> Result<u64, ApiFailure> {
    let until = parse_until(&intent.until)?;
    let target = mailbox_with_role(app, &intent.account_id, MailboxRole::Snooze)?;
    // The move first: the mailbox replace clears any prior snooze row (the
    // store invariant), then the new return time is recorded.
    let ack = app
        .service
        .replace_mailboxes(
            &intent.account_id,
            &intent.message_id,
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![target],
            },
        )
        .await?;
    let store = app.store.clone();
    let account_id = intent.account_id.clone();
    let message_id = intent.message_id.clone();
    offload_read(move || Ok(store.insert_snooze(&account_id, &message_id, until)?)).await?;
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

pub(crate) async fn unsnooze(app: &AppState, intent: UnsnoozeIntent) -> Result<u64, ApiFailure> {
    let target = mailbox_with_role(app, &intent.account_id, MailboxRole::Inbox)?;
    // The replace clears the snooze row in the same stroke (store invariant:
    // leaving the snoozed mailbox deletes the row).
    let ack = app
        .service
        .replace_mailboxes(
            &intent.account_id,
            &intent.message_id,
            &ReplaceMailboxesCommand {
                mailbox_ids: vec![target],
            },
        )
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

/// The account's mailbox carrying `role`, or a conflict: snoozing needs a
/// designated snooze mailbox (assigned through `setMailboxRole`), and
/// unsnoozing needs an inbox to return to.
fn mailbox_with_role(
    app: &AppState,
    account_id: &AccountId,
    role: MailboxRole,
) -> Result<MailboxId, ApiFailure> {
    scoped_accounts(app, Some(account_id))?;
    app.service
        .list_mailboxes(account_id)?
        .into_iter()
        .find(|mailbox| mailbox.role.as_deref() == Some(role.as_str()))
        .map(|mailbox| mailbox.id)
        .ok_or_else(|| {
            ApiFailure::new(
                StatusCode::CONFLICT,
                ApiErrorKind::Conflict,
                format!("no mailbox has the {} role", role.as_str()),
                false,
            )
        })
}

/// Parse the wall-clock return time (RFC 3339) into the unix seconds the
/// snooze store keeps.
fn parse_until(until: &str) -> Result<i64, ApiFailure> {
    time::OffsetDateTime::parse(until, &time::format_description::well_known::Rfc3339)
        .map(time::OffsetDateTime::unix_timestamp)
        .map_err(|error| ApiFailure::malformed(format!("invalid snooze time: {error}")))
}
