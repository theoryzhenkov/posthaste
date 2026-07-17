//! Mail-mutation commands: keywords, mailbox membership, destroy.

use posthaste_client_models::{DestroyMessageIntent, ReplaceMailboxesIntent, SetKeywordsIntent};

use super::command::finish_mail_command;
use super::ApiFailure;
use crate::AppState;

pub(crate) async fn set_keywords(
    app: &AppState,
    intent: SetKeywordsIntent,
) -> Result<u64, ApiFailure> {
    let ack = app
        .service
        .set_keywords(&intent.account_id, &intent.message_id, &intent.change)
        .await?;
    Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
}

pub(crate) async fn replace_mailboxes(
    app: &AppState,
    intent: ReplaceMailboxesIntent,
) -> Result<u64, ApiFailure> {
    let ack = app
        .service
        .replace_mailboxes(&intent.account_id, &intent.message_id, &intent.change)
        .await?;
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
