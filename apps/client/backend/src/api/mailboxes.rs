//! The mailbox family: counts, provider-mailbox create/rename/delete, and
//! role assignment. Mailbox mutations are synchronous, not optimistic: a
//! blocking provider round-trip applies the change and a resync reads it
//! back, so the reply's generation already reflects the provider's answer.

use posthaste_client_models::{
    CreateMailboxIntent, DeleteMailboxIntent, MailboxCountsQuery, MailboxCountsResult,
    MailboxCountsRow, RenameMailboxIntent, SetMailboxRoleIntent,
};
use posthaste_domain_model::{AccountId, DomainEvent, MailboxRole};
use posthaste_domain_service::SharedGateway;

use super::{scoped_accounts, ApiFailure};
use crate::AppState;

pub(crate) fn evaluate_mailbox_counts(
    app: &AppState,
    query: MailboxCountsQuery,
) -> Result<MailboxCountsResult, ApiFailure> {
    let mut rows = Vec::new();
    for account_id in scoped_accounts(app, query.account_id.as_ref())? {
        let mut mailboxes = app.service.list_mailboxes(&account_id)?;
        // Display order is part of the answer — role mailboxes first in a
        // fixed precedence, then named folders by name — so the client
        // renders the rows verbatim instead of re-sorting a query answer.
        mailboxes.sort_by(|left, right| {
            mailbox_role_rank(left.role.as_deref())
                .cmp(&mailbox_role_rank(right.role.as_deref()))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        rows.extend(mailboxes.into_iter().map(|mailbox| MailboxCountsRow {
            account_id: account_id.clone(),
            mailbox,
        }));
    }
    Ok(MailboxCountsResult { rows })
}

/// Fixed display precedence for role mailboxes; named folders follow, by
/// name.
fn mailbox_role_rank(role: Option<&str>) -> u8 {
    match role {
        Some("inbox") => 0,
        Some("drafts") => 1,
        Some("sent") => 2,
        Some("archive") => 3,
        Some("junk") => 4,
        Some("trash") => 5,
        _ => 6,
    }
}

pub(crate) async fn create_mailbox(
    app: &AppState,
    intent: CreateMailboxIntent,
) -> Result<u64, ApiFailure> {
    let name = intent.name.trim();
    if name.is_empty() {
        return Err(ApiFailure::malformed("mailbox name must not be empty"));
    }
    let gateway = connected_gateway(app, &intent.account_id).await?;
    let events = app
        .service
        .create_mailbox(&intent.account_id, name, gateway.as_ref())
        .await?;
    Ok(publish_events(app, events))
}

pub(crate) async fn rename_mailbox(
    app: &AppState,
    intent: RenameMailboxIntent,
) -> Result<u64, ApiFailure> {
    let name = intent.name.trim();
    if name.is_empty() {
        return Err(ApiFailure::malformed("mailbox name must not be empty"));
    }
    let gateway = connected_gateway(app, &intent.account_id).await?;
    // IMAP accounts refuse the rename with a typed rejection (their ids
    // encode the mailbox name); JMAP applies a name-only update.
    let events = app
        .service
        .rename_mailbox(
            &intent.account_id,
            &intent.mailbox_id,
            name,
            gateway.as_ref(),
        )
        .await?;
    Ok(publish_events(app, events))
}

pub(crate) async fn delete_mailbox(
    app: &AppState,
    intent: DeleteMailboxIntent,
) -> Result<u64, ApiFailure> {
    let gateway = connected_gateway(app, &intent.account_id).await?;
    // A non-empty mailbox without the confirmed flag is refused as a
    // conflict by the service, before the provider is touched.
    let events = app
        .service
        .destroy_mailbox(
            &intent.account_id,
            &intent.mailbox_id,
            intent.remove_emails,
            gateway.as_ref(),
        )
        .await?;
    Ok(publish_events(app, events))
}

pub(crate) async fn set_mailbox_role(
    app: &AppState,
    intent: SetMailboxRoleIntent,
) -> Result<u64, ApiFailure> {
    // Normalize the role against the vocabulary before touching the
    // provider; `None` clears. The Posthaste-local `snooze` role is valid
    // here — the service writes it as a local override without a provider
    // round-trip.
    let role = match intent.role.as_deref().map(str::trim) {
        None => None,
        Some("") => None,
        Some(role) => Some(MailboxRole::parse(role).ok_or_else(|| {
            ApiFailure::malformed(format!("'{role}' is not a known mailbox role"))
        })?),
    };
    let gateway = connected_gateway(app, &intent.account_id).await?;
    let events = app
        .service
        .set_mailbox_role(
            &intent.account_id,
            &intent.mailbox_id,
            role.map(MailboxRole::as_str),
            gateway.as_ref(),
        )
        .await?;
    Ok(publish_events(app, events))
}

/// The live gateway for an account: an unknown account is an unknown-id
/// failure; a known but disconnected one cannot take a synchronous mailbox
/// mutation and fails as unavailable.
async fn connected_gateway(
    app: &AppState,
    account_id: &AccountId,
) -> Result<SharedGateway, ApiFailure> {
    if app.service.get_source(account_id)?.is_none() {
        return Err(ApiFailure::unknown_id(format!(
            "account {}",
            account_id.as_str()
        )));
    }
    app.supervisor.gateway(account_id).await.map_err(|_| {
        ApiFailure::unavailable(format!(
            "account {} is not connected; mailbox changes need a live provider connection",
            account_id.as_str()
        ))
    })
}

/// Publish a mailbox mutation's committed events (bumping the generation; an
/// event-less commit still bumps) and return the resulting generation. No
/// sync nudge — the service already resynced before returning.
fn publish_events(app: &AppState, events: Vec<DomainEvent>) -> u64 {
    if events.is_empty() {
        app.events.bump()
    } else {
        app.events.publish(&events);
        app.events.generation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_role_rank_orders_roles_before_named_folders() {
        assert!(mailbox_role_rank(Some("inbox")) < mailbox_role_rank(Some("drafts")));
        assert!(mailbox_role_rank(Some("drafts")) < mailbox_role_rank(Some("sent")));
        assert!(mailbox_role_rank(Some("trash")) < mailbox_role_rank(Some("Projects")));
        assert_eq!(mailbox_role_rank(None), mailbox_role_rank(Some("unknown")));
    }
}
