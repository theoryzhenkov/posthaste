//! The compose family: draft commands, send, and the sender-address
//! autocomplete corpus.

use std::collections::BTreeSet;

use posthaste_client_models::{
    CreateDraftIntent, DiscardDraftIntent, SendIntent, SenderAddressRow, SenderAddressesQuery,
    SenderAddressesResult, UpdateDraftIntent,
};
use posthaste_domain_model::{AccountId, MessageId, OperationId};

use super::command::finish_mail_command;
use super::{scoped_accounts, ApiFailure};
use crate::AppState;

pub(crate) async fn create_draft(
    app: &AppState,
    intent: CreateDraftIntent,
) -> Result<u64, ApiFailure> {
    let draft_key = intent
        .draft
        .draft_id
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(MessageId::from);
    let (_, events) = app
        .service
        .save_draft(&intent.account_id, draft_key, intent.draft)
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, events).await)
}

pub(crate) async fn update_draft(
    app: &AppState,
    intent: UpdateDraftIntent,
) -> Result<u64, ApiFailure> {
    let (_, events) = app
        .service
        .save_draft(
            &intent.account_id,
            Some(MessageId::from(intent.draft_id.as_str())),
            intent.draft,
        )
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, events).await)
}

pub(crate) async fn discard_draft(
    app: &AppState,
    intent: DiscardDraftIntent,
) -> Result<u64, ApiFailure> {
    let ack = app
        .service
        .discard_draft(
            &intent.account_id,
            MessageId::from(intent.draft_id.as_str()),
        )
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

pub(crate) async fn send(
    app: &AppState,
    command_id: &str,
    intent: SendIntent,
) -> Result<u64, ApiFailure> {
    // The command id becomes the outbox operation id, so the intent id is
    // the send's idempotency key end to end — a replayed id can never
    // enqueue a second dispatch.
    let (_, events) = app
        .service
        .enqueue_send_with_operation_id(
            &intent.account_id,
            intent.request,
            Some(OperationId::from(command_id)),
        )
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, events).await)
}

pub(crate) fn evaluate_sender_addresses(
    app: &AppState,
    query: SenderAddressesQuery,
) -> Result<SenderAddressesResult, ApiFailure> {
    let accounts = scoped_accounts(app, query.account_id.as_ref())?;
    let allowed: BTreeSet<&str> = accounts.iter().map(AccountId::as_str).collect();
    let mut rows: Vec<SenderAddressRow> = app
        .store
        .list_sender_address_cache()?
        .into_iter()
        .filter(|cached| allowed.contains(cached.source_id.as_str()))
        .map(|cached| SenderAddressRow {
            account_id: cached.source_id,
            name: cached.name,
            email: cached.email,
            last_used_at: cached.last_used_at,
        })
        .collect();
    // Most recently used first; last_used_at is normalized RFC 3339, so the
    // lexicographic order is the chronological order.
    rows.sort_by(|left, right| right.last_used_at.cmp(&left.last_used_at));
    Ok(SenderAddressesResult { rows })
}
