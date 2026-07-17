//! `POST /query`: decode one typed read and route it to its family module.

use axum::body::Bytes;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use posthaste_client_models::{Query, QueryEnvelope};

use super::{
    accounts, automation, compose, decode_json, mail_list, mailboxes, message_detail, offload_read,
    operations, rev_log, settings, smart_mailboxes, tags, thread, to_value, ApiFailure, ApiState,
};
use crate::AppState;

/// `POST /query`: evaluate one typed read over the effective views. The
/// generation is stamped BEFORE evaluation, so a write racing the read makes
/// the answer look older than the stream and the client refetches — staleness
/// always resolves toward a refetch, never a stuck view.
pub(crate) async fn handle_query(
    State(state): State<ApiState>,
    body: Bytes,
) -> Result<Response, ApiFailure> {
    let query: Query = decode_json(&body)?;
    let generation = state.app.events.generation();
    let data = evaluate_query(&state.app, query).await?;
    Ok(Json(QueryEnvelope { generation, data }).into_response())
}

/// One arm per family; synchronous store reads go through
/// [`offload_read`](super::offload_read) inside the family evaluators.
async fn evaluate_query(app: &AppState, query: Query) -> Result<serde_json::Value, ApiFailure> {
    let data = match query {
        Query::MailList(query) => {
            let app = app.clone();
            to_value(offload_read(move || mail_list::evaluate_mail_list(&app, query)).await?)?
        }
        Query::Thread(query) => to_value(thread::evaluate_thread(app, query).await?)?,
        Query::MessageDetail(query) => {
            to_value(message_detail::evaluate_message_detail(app, query).await?)?
        }
        Query::MessageRawSource(query) => {
            to_value(message_detail::evaluate_message_raw_source(app, query).await?)?
        }
        Query::MailboxCounts(query) => {
            let app = app.clone();
            to_value(offload_read(move || mailboxes::evaluate_mailbox_counts(&app, query)).await?)?
        }
        Query::Accounts(_) => to_value(accounts::evaluate_accounts(app).await?)?,
        Query::AccountSettings(query) => {
            to_value(accounts::evaluate_account_settings(app, query)?)?
        }
        Query::VerifyAccount(query) => {
            to_value(accounts::evaluate_verify_account(app, query).await?)?
        }
        Query::OauthStart(query) => to_value(accounts::evaluate_oauth_start(app, query)?)?,
        Query::PendingOperations(query) => {
            let app = app.clone();
            to_value(
                offload_read(move || operations::evaluate_pending_operations(&app, query)).await?,
            )?
        }
        Query::AppSettings(query) => to_value(settings::evaluate_app_settings(app, query)?)?,
        Query::SmartMailboxes(query) => {
            let app = app.clone();
            to_value(
                offload_read(move || smart_mailboxes::evaluate_smart_mailboxes(&app, query))
                    .await?,
            )?
        }
        Query::Tags(query) => {
            let app = app.clone();
            to_value(offload_read(move || tags::evaluate_tags(&app, query)).await?)?
        }
        Query::AutomationRulePreview(query) => {
            let app = app.clone();
            to_value(offload_read(move || automation::evaluate_rule_preview(&app, query)).await?)?
        }
        Query::RevLog(query) => {
            let app = app.clone();
            to_value(offload_read(move || rev_log::evaluate_rev_log(&app, query)).await?)?
        }
        Query::SenderAddresses(query) => {
            let app = app.clone();
            to_value(offload_read(move || compose::evaluate_sender_addresses(&app, query)).await?)?
        }
    };
    Ok(data)
}
