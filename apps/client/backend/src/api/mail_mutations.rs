//! Mail-mutation commands: keywords, mailbox membership, destroy.
//!
//! The two reversible mutations (keywords, mailboxes) RECORD themselves in
//! the per-account rev-log as they apply: the effective before/after delta is
//! captured server-side and appended with the cursor auto-advance (redo tail
//! truncated), which is what `undo`/`redo` (rev_log.rs) later replay. The
//! recording moved here when the split-model stack was retired — its server
//! appended client-captured diffs on confirmation; the integrated backend is
//! the only writer, so it captures the diff itself. `destroy` is not
//! reversible and records nothing.

use std::collections::BTreeSet;

use posthaste_client_models::{DestroyMessageIntent, ReplaceMailboxesIntent, SetKeywordsIntent};
use posthaste_domain_model::{now_iso8601, AccountId, Id, MessageId};

use super::command::finish_mail_command;
use super::{offload_read, ApiFailure};
use crate::AppState;

pub(crate) async fn set_keywords(
    app: &AppState,
    intent: SetKeywordsIntent,
) -> Result<u64, ApiFailure> {
    // Snapshot BEFORE the apply: the recorded delta is the EFFECTIVE change
    // (adding a keyword the message already carries must not undo to a
    // removal), so it is computed against the pre-apply state. An implicit
    // gesture (`record_undo: false` — the auto-mark-read) skips the log.
    let before = intent
        .record_undo
        .unwrap_or(true)
        .then(|| effective_state(app, &intent.account_id, &intent.message_id))
        .flatten();
    let ack = app
        .service
        .set_keywords(&intent.account_id, &intent.message_id, &intent.change)
        .await?;
    if let Some((current_keywords, _)) = before {
        let current: BTreeSet<&str> = current_keywords.iter().map(String::as_str).collect();
        let added: Vec<&String> = intent
            .change
            .add
            .iter()
            .filter(|keyword| !current.contains(keyword.as_str()))
            .collect();
        let removed: Vec<&String> = intent
            .change
            .remove
            .iter()
            .filter(|keyword| current.contains(keyword.as_str()))
            .collect();
        record_step(
            app,
            &intent.account_id,
            &intent.message_id,
            serde_json::json!({
                "keywords": { "added": added, "removed": removed },
                "mailboxes": { "added": [], "removed": [] },
            }),
            added.is_empty() && removed.is_empty(),
        )
        .await;
    }
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

pub(crate) async fn replace_mailboxes(
    app: &AppState,
    intent: ReplaceMailboxesIntent,
) -> Result<u64, ApiFailure> {
    let before = effective_state(app, &intent.account_id, &intent.message_id);
    let ack = app
        .service
        .replace_mailboxes(&intent.account_id, &intent.message_id, &intent.change)
        .await?;
    if let Some((_, current_mailboxes)) = before {
        let current: BTreeSet<&str> = current_mailboxes
            .iter()
            .map(|mailbox_id| mailbox_id.as_str())
            .collect();
        let target: BTreeSet<&str> = intent
            .change
            .mailbox_ids
            .iter()
            .map(|mailbox_id| mailbox_id.as_str())
            .collect();
        let added: Vec<&str> = target.difference(&current).copied().collect();
        let removed: Vec<&str> = current.difference(&target).copied().collect();
        record_step(
            app,
            &intent.account_id,
            &intent.message_id,
            serde_json::json!({
                "keywords": { "added": [], "removed": [] },
                "mailboxes": { "added": added, "removed": removed },
            }),
            added.is_empty() && removed.is_empty(),
        )
        .await;
    }
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

pub(crate) async fn destroy(
    app: &AppState,
    intent: DestroyMessageIntent,
) -> Result<u64, ApiFailure> {
    let ack = app
        .service
        .destroy_message(&intent.account_id, &intent.message_id)
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

/// The message's pre-apply effective (keywords, mailboxes) — `None` when the
/// message has no visible row (recording is then skipped; the mutation itself
/// decides whether that is an error).
fn effective_state(
    app: &AppState,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Option<(Vec<String>, Vec<posthaste_domain_model::MailboxId>)> {
    let summary = app
        .service
        .get_message_header(account_id, message_id)
        .ok()
        .flatten()?
        .summary;
    Some((summary.keywords, summary.mailbox_ids))
}

/// Append the applied mutation to the rev-log (cursor auto-advances, redo
/// tail truncates in the same store transaction). An EMPTY delta records
/// nothing — a no-op action must not clobber the redo tail. Recording is
/// best-effort: the mutation is already committed and queued, so a failed
/// append only warns (the action simply is not undoable).
async fn record_step(
    app: &AppState,
    account_id: &AccountId,
    message_id: &MessageId,
    diff: serde_json::Value,
    is_empty: bool,
) {
    if is_empty {
        return;
    }
    let store = app.store.clone();
    let owned_account = account_id.clone();
    let owned_message = message_id.clone();
    let step_id = Id::generate().to_string();
    let created_at = match now_iso8601() {
        Ok(now) => now,
        Err(error) => {
            tracing::warn!(%error, "rev-log step skipped: clock failed");
            return;
        }
    };
    let result = offload_read(move || {
        Ok(store.append_rev_log_step(
            &owned_account,
            &step_id,
            owned_message.as_str(),
            owned_account.as_str(),
            &diff,
            &created_at,
        )?)
    })
    .await;
    if let Err(error) = result {
        tracing::warn!(
            account_id = %account_id,
            message_id = %message_id,
            error = %error.error.message,
            "rev-log step append failed; the action will not be undoable"
        );
    }
}
