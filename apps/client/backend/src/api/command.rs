//! `POST /command`: idempotency, dispatch to the family appliers, and the
//! shared finish path for mail mutations.

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use posthaste_client_models::{Command, CommandAccepted, CommandEnvelope};
use posthaste_domain_model::{AccountId, DomainEvent, OperationId, SyncTrigger};

use super::{
    accounts, automation, compose, decode_json, mail_mutations, mailboxes, offload_read,
    operations, rev_log, settings, smart_mailboxes, snooze, sync, unsubscribe, ApiFailure,
    ApiState, CommandOutcome, COMMAND_OUTCOME_CAP,
};
use crate::AppState;

/// `POST /command`: apply one typed intent. Replaying an id returns an
/// outcome without re-applying: concurrent and in-run retries resolve
/// against the per-id outcome cell, and a send retry that outlives the
/// process resolves against the outbox, whose operation id for a send is
/// the command id itself.
pub(crate) async fn handle_command(
    State(state): State<ApiState>,
    body: Bytes,
) -> Result<Json<CommandAccepted>, ApiFailure> {
    let envelope: CommandEnvelope = decode_json(&body)?;
    if envelope.id.trim().is_empty() {
        return Err(ApiFailure::malformed("command id must not be empty"));
    }
    let cell = command_outcome_cell(&state, &envelope.id);
    let mut outcome = cell.lock().await;
    if let Some(generation) = *outcome {
        return Ok(Json(CommandAccepted { generation }));
    }
    if let Some(generation) = replay_durable_outcome(&state, &envelope).await? {
        *outcome = Some(generation);
        return Ok(Json(CommandAccepted { generation }));
    }
    let generation = apply_command(&state.app, &envelope.id, envelope.command).await?;
    *outcome = Some(generation);
    Ok(Json(CommandAccepted { generation }))
}

/// The outcome cell for one command id. The map lock is held only for the
/// lookup, so distinct commands execute concurrently; retries of one id
/// share the cell and serialize on it. Once the map outgrows its cap,
/// settled cells are evicted (an in-flight cell is also held by its
/// executing request, so it survives the sweep).
fn command_outcome_cell(state: &ApiState, id: &str) -> CommandOutcome {
    let mut outcomes = state
        .command_outcomes
        .lock()
        .expect("command-outcome map lock poisoned");
    if outcomes.len() >= COMMAND_OUTCOME_CAP && !outcomes.contains_key(id) {
        outcomes.retain(|_, cell| std::sync::Arc::strong_count(cell) > 1);
    }
    outcomes.entry(id.to_string()).or_default().clone()
}

/// Durable replay detection for a send: its outbox operation id is the
/// command id, so a replayed id whose operation the outbox still holds is
/// answered from the stored intent without enqueuing a second send. The
/// current generation is at or past the original acceptance, which is all
/// the reply promises.
async fn replay_durable_outcome(
    state: &ApiState,
    envelope: &CommandEnvelope,
) -> Result<Option<u64>, ApiFailure> {
    if !matches!(envelope.command, Command::Send(_)) {
        return Ok(None);
    }
    let service = state.app.service.clone();
    let operation_id = OperationId::from(envelope.id.as_str());
    let existing = offload_read(move || Ok(service.get_operation(&operation_id)?)).await?;
    Ok(existing.map(|_| state.app.events.generation()))
}

/// One arm per family; every arm delegates to its family module.
async fn apply_command(
    app: &AppState,
    command_id: &str,
    command: Command,
) -> Result<u64, ApiFailure> {
    match command {
        Command::SetKeywords(intent) => mail_mutations::set_keywords(app, intent).await,
        Command::ReplaceMailboxes(intent) => mail_mutations::replace_mailboxes(app, intent).await,
        Command::Destroy(intent) => mail_mutations::destroy(app, intent).await,
        Command::CreateDraft(intent) => compose::create_draft(app, intent).await,
        Command::UpdateDraft(intent) => compose::update_draft(app, intent).await,
        Command::DiscardDraft(intent) => compose::discard_draft(app, intent).await,
        Command::Send(intent) => compose::send(app, command_id, intent).await,
        Command::CreateAccount(intent) => accounts::create_account(app, intent).await,
        Command::UpdateAccount(intent) => accounts::update_account(app, intent).await,
        Command::UpdateAccountTransport(intent) => {
            accounts::update_account_transport(app, intent).await
        }
        Command::SetAccountSecret(intent) => accounts::set_account_secret(app, intent).await,
        Command::DeleteAccount(intent) => accounts::delete_account(app, intent).await,
        Command::SetAccountLogo(intent) => accounts::set_account_logo(app, intent).await,
        Command::CompleteOauth(intent) => accounts::complete_oauth(app, intent).await,
        Command::UpdateSettings(intent) => settings::update_settings(app, intent).await,
        Command::CreateSmartMailbox(intent) => {
            smart_mailboxes::create_smart_mailbox(app, intent).await
        }
        Command::UpdateSmartMailbox(intent) => {
            smart_mailboxes::update_smart_mailbox(app, intent).await
        }
        Command::DeleteSmartMailbox(intent) => {
            smart_mailboxes::delete_smart_mailbox(app, intent).await
        }
        Command::ResetSmartMailboxes(intent) => {
            smart_mailboxes::reset_smart_mailboxes(app, intent).await
        }
        Command::CreateMailbox(intent) => mailboxes::create_mailbox(app, intent).await,
        Command::RenameMailbox(intent) => mailboxes::rename_mailbox(app, intent),
        Command::DeleteMailbox(intent) => mailboxes::delete_mailbox(app, intent).await,
        Command::SetMailboxRole(intent) => mailboxes::set_mailbox_role(app, intent).await,
        Command::CreateAutomationRule(intent) => automation::create_rule(app, intent).await,
        Command::UpdateAutomationRule(intent) => automation::update_rule(app, intent).await,
        Command::DeleteAutomationRule(intent) => automation::delete_rule(app, intent).await,
        Command::Snooze(intent) => snooze::snooze(app, intent).await,
        Command::Unsnooze(intent) => snooze::unsnooze(app, intent).await,
        Command::Undo(intent) => rev_log::undo(app, intent).await,
        Command::Redo(intent) => rev_log::redo(app, intent).await,
        Command::SyncNow(intent) => sync::sync_now(app, intent),
        Command::RetryOperation(intent) => operations::retry_operation(app, intent).await,
        Command::CancelOperation(intent) => operations::cancel_operation(app, intent).await,
        Command::Unsubscribe(intent) => unsubscribe::unsubscribe(app, intent),
    }
}

/// Publish a mail command's committed-write events (bumping the generation;
/// an event-less commit still bumps) and nudge the account runtime so the
/// queued operation flushes promptly. A missing runtime is fine — the op is
/// durable and flushes on the next sync window.
pub(crate) async fn finish_mail_command(
    app: &AppState,
    account_id: &AccountId,
    events: Vec<DomainEvent>,
) -> u64 {
    let generation = if events.is_empty() {
        app.events.bump()
    } else {
        app.events.publish(&events);
        app.events.generation()
    };
    let _ = app
        .supervisor
        .trigger_account_sync(account_id, SyncTrigger::Manual)
        .await;
    generation
}
