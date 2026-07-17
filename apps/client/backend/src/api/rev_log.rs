//! The rev-log family: the per-account undo/redo history read, and the
//! cursor-move commands. `undo` reverts the step at the cursor by applying
//! the inverse of its recorded diff through the ordinary mutation path (so
//! the revert is itself an outbox operation), then moves the cursor down;
//! `redo` re-applies the head of the redo tail and moves the cursor up.

use std::collections::BTreeSet;

use axum::http::StatusCode;
use posthaste_client_models::{ApiErrorKind, RedoIntent, RevLogQuery, RevLogResult, UndoIntent};
use posthaste_domain_model::{
    AccountId, DomainEvent, MailboxId, MessageId, ReplaceMailboxesCommand, RevLogSnapshot,
    RevLogStep, SetKeywordsCommand,
};
use serde::Deserialize;

use super::command::finish_mail_command;
use super::{offload_read, scoped_accounts, ApiFailure};
use crate::AppState;

/// The two-facet change a rev-log step records (`MessageChangeDiff` JSON):
/// what the forward action added and removed, per facet.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct StepDiff {
    keywords: FacetDelta,
    mailboxes: FacetDelta,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct FacetDelta {
    added: Vec<String>,
    removed: Vec<String>,
}

impl StepDiff {
    /// The diff that reverses this one: added and removed swap, per facet.
    fn inverse(&self) -> Self {
        Self {
            keywords: self.keywords.inverse(),
            mailboxes: self.mailboxes.inverse(),
        }
    }
}

impl FacetDelta {
    fn inverse(&self) -> Self {
        Self {
            added: self.removed.clone(),
            removed: self.added.clone(),
        }
    }

    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

pub(crate) fn evaluate_rev_log(
    app: &AppState,
    query: RevLogQuery,
) -> Result<RevLogResult, ApiFailure> {
    scoped_accounts(app, Some(&query.account_id))?;
    let snapshot = app.store.rev_log_snapshot(&query.account_id)?;
    Ok(RevLogResult {
        steps: snapshot.steps,
        cursor: snapshot.cursor,
    })
}

pub(crate) async fn undo(app: &AppState, intent: UndoIntent) -> Result<u64, ApiFailure> {
    let snapshot = read_snapshot(app, &intent.account_id).await?;
    let Some(cursor_step_id) = snapshot.cursor.cursor_step_id.clone() else {
        return Err(cursor_conflict("nothing to undo"));
    };
    let step = find_step(&snapshot, &cursor_step_id)?;
    let diff = decode_diff(step)?;
    let events = apply_diff(app, &intent.account_id, step, &diff.inverse()).await?;
    // Cursor down one: the predecessor step becomes the topmost applied one,
    // and the undone step joins the head of the redo tail (it holds the
    // lowest seq of the undone range, so seq order is preserved).
    let predecessor = snapshot
        .steps
        .iter()
        .filter(|candidate| candidate.seq < step.seq)
        .max_by_key(|candidate| candidate.seq)
        .map(|candidate| candidate.step_id.clone());
    let mut redo_tail = vec![cursor_step_id];
    redo_tail.extend(snapshot.cursor.redo_tail.iter().cloned());
    write_cursor(app, &intent.account_id, predecessor, redo_tail).await?;
    Ok(finish_mail_command(app, &intent.account_id, events).await)
}

pub(crate) async fn redo(app: &AppState, intent: RedoIntent) -> Result<u64, ApiFailure> {
    let snapshot = read_snapshot(app, &intent.account_id).await?;
    let Some(next_step_id) = snapshot.cursor.redo_tail.first().cloned() else {
        return Err(cursor_conflict("nothing to redo"));
    };
    let step = find_step(&snapshot, &next_step_id)?;
    let diff = decode_diff(step)?;
    let events = apply_diff(app, &intent.account_id, step, &diff).await?;
    let redo_tail = snapshot.cursor.redo_tail[1..].to_vec();
    write_cursor(app, &intent.account_id, Some(next_step_id), redo_tail).await?;
    Ok(finish_mail_command(app, &intent.account_id, events).await)
}

/// Apply one diff, forward, to the step's message: keywords through
/// `setKeywords`, mailbox membership as a replace computed against the
/// current effective membership. Callers pass the recorded diff for redo and
/// its inverse for undo.
async fn apply_diff(
    app: &AppState,
    account_id: &AccountId,
    step: &RevLogStep,
    diff: &StepDiff,
) -> Result<Vec<DomainEvent>, ApiFailure> {
    let message_id = MessageId::from(step.message_id.as_str());
    let mut events = Vec::new();
    if !diff.keywords.is_empty() {
        let ack = app
            .service
            .set_keywords(
                account_id,
                &message_id,
                &SetKeywordsCommand {
                    add: diff.keywords.added.clone(),
                    remove: diff.keywords.removed.clone(),
                },
            )
            .await?;
        events.extend(ack.events);
    }
    if !diff.mailboxes.is_empty() {
        let current = app
            .service
            .get_message_header(account_id, &message_id)?
            .ok_or_else(|| ApiFailure::unknown_id(format!("message {}", message_id.as_str())))?
            .summary
            .mailbox_ids;
        let removed: BTreeSet<&str> = diff.mailboxes.removed.iter().map(String::as_str).collect();
        let mut target: Vec<MailboxId> = current
            .into_iter()
            .filter(|mailbox_id| !removed.contains(mailbox_id.as_str()))
            .collect();
        for added in &diff.mailboxes.added {
            if !target.iter().any(|mailbox_id| mailbox_id.as_str() == added) {
                target.push(MailboxId::from(added.as_str()));
            }
        }
        let ack = app
            .service
            .replace_mailboxes(
                account_id,
                &message_id,
                &ReplaceMailboxesCommand {
                    mailbox_ids: target,
                },
            )
            .await?;
        events.extend(ack.events);
    }
    Ok(events)
}

async fn read_snapshot(
    app: &AppState,
    account_id: &AccountId,
) -> Result<RevLogSnapshot, ApiFailure> {
    scoped_accounts(app, Some(account_id))?;
    let store = app.store.clone();
    let account_id = account_id.clone();
    offload_read(move || Ok(store.rev_log_snapshot(&account_id)?)).await
}

async fn write_cursor(
    app: &AppState,
    account_id: &AccountId,
    cursor_step_id: Option<String>,
    redo_tail: Vec<String>,
) -> Result<(), ApiFailure> {
    let store = app.store.clone();
    let account_id = account_id.clone();
    offload_read(move || {
        Ok(store.set_rev_cursor(&account_id, cursor_step_id.as_deref(), &redo_tail)?)
    })
    .await
}

fn find_step<'snapshot>(
    snapshot: &'snapshot RevLogSnapshot,
    step_id: &str,
) -> Result<&'snapshot RevLogStep, ApiFailure> {
    snapshot
        .steps
        .iter()
        .find(|step| step.step_id == step_id)
        .ok_or_else(|| {
            ApiFailure::internal(format!("rev-log cursor references missing step {step_id}"))
        })
}

fn decode_diff(step: &RevLogStep) -> Result<StepDiff, ApiFailure> {
    serde_json::from_value(step.diff.clone()).map_err(|error| {
        ApiFailure::internal(format!(
            "rev-log step {} holds an undecodable diff: {error}",
            step.step_id
        ))
    })
}

fn cursor_conflict(message: &str) -> ApiFailure {
    ApiFailure::new(StatusCode::CONFLICT, ApiErrorKind::Conflict, message, false)
}
