use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::{compose, db, jmap, sanitize};
use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxResponse {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailResponse {
    pub id: String,
    pub thread_id: String,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: String, // ISO 8601
    pub has_attachment: bool,
    pub is_read: bool,
    pub is_flagged: bool,
    pub mailbox_ids: Vec<String>,
    pub keywords: Vec<String>,
}

fn email_row_to_response(row: db::EmailRow) -> EmailResponse {
    EmailResponse {
        id: row.id,
        thread_id: row.thread_id,
        subject: row.subject,
        from_name: row.from_name,
        from_email: row.from_email,
        preview: row.preview,
        received_at: unix_to_iso8601(row.received_at),
        has_attachment: row.has_attachment,
        is_read: row.is_read,
        is_flagged: row.is_flagged,
        mailbox_ids: row.mailbox_ids,
        keywords: row.keywords,
    }
}

/// Convert unix timestamp to ISO 8601 string without external crate.
fn unix_to_iso8601(ts: i64) -> String {
    // Compute date/time from unix timestamp (UTC)
    let secs_per_day: i64 = 86400;
    let days = ts.div_euclid(secs_per_day);
    let day_secs = ts.rem_euclid(secs_per_day);

    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Days since 1970-01-01 to y/m/d (civil_from_days algorithm)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

pub async fn list_mailboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<MailboxResponse>>, StatusCode> {
    tokio::task::spawn_blocking(move || {
        let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = db::get_mailboxes(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let response: Vec<MailboxResponse> = rows
            .into_iter()
            .map(|r| MailboxResponse {
                id: r.id,
                name: r.name,
                role: r.role,
                unread_emails: r.unread_emails,
                total_emails: r.total_emails,
            })
            .collect();
        Ok(Json(response))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn list_emails_in_mailbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EmailResponse>>, StatusCode> {
    tokio::task::spawn_blocking(move || {
        let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = db::get_emails_in_mailbox(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(rows.into_iter().map(email_row_to_response).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn list_all_emails(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<EmailResponse>>, StatusCode> {
    tokio::task::spawn_blocking(move || {
        let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = db::get_all_emails(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(rows.into_iter().map(email_row_to_response).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn get_email(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<EmailResponse>, StatusCode> {
    tokio::task::spawn_blocking(move || {
        let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        match db::get_email(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            Some(row) => Ok(Json(email_row_to_response(row))),
            None => Err(StatusCode::NOT_FOUND),
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub async fn get_thread(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EmailResponse>>, StatusCode> {
    tokio::task::spawn_blocking(move || {
        let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let rows = db::get_thread(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(Json(rows.into_iter().map(email_row_to_response).collect()))
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailBodyResponse {
    pub email_id: String,
    pub html: Option<String>,
    pub text: Option<String>,
}

pub async fn get_email_body(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<EmailBodyResponse>, StatusCode> {
    // 1. Check cache
    let cached = {
        let state = state.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            db::get_email_body(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??
    };

    if let Some(body) = cached {
        return Ok(Json(EmailBodyResponse {
            email_id: body.email_id,
            html: body.html,
            text: body.text_body,
        }));
    }

    // 2. Fetch from JMAP
    let client = state
        .jmap_client
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let (raw_html, text) = jmap::fetch_email_body(client, &id).await.map_err(|e| {
        eprintln!("Failed to fetch email body: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    // 3. Sanitize HTML
    let sanitized_html = raw_html.map(|h| sanitize::sanitize_email_html(&h));

    // 4. Cache
    {
        let state = state.clone();
        let id = id.clone();
        let html = sanitized_html.clone();
        let text = text.clone();
        tokio::task::spawn_blocking(move || {
            let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            db::save_email_body(&conn, &id, html.as_deref(), text.as_deref())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;
    }

    // 5. Return
    Ok(Json(EmailBodyResponse {
        email_id: id,
        html: sanitized_html,
        text,
    }))
}

// --- Email mutations ---

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum EmailActionRequest {
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    Archive,
    Trash,
    Delete,
    Move { mailbox_id: String },
}

/// Describes a mutation to apply: keyword change, mailbox move, or destroy.
enum EmailMutation {
    Keyword { keyword: &'static str, set: bool },
    MoveTo { mailbox_id: String },
    Destroy,
}

impl EmailActionRequest {
    /// Convert the action into a concrete mutation, resolving mailbox IDs for
    /// archive/trash via the local DB.
    fn into_mutation(
        self,
        conn: &rusqlite::Connection,
    ) -> Result<EmailMutation, StatusCode> {
        match self {
            Self::MarkRead => Ok(EmailMutation::Keyword { keyword: "$seen", set: true }),
            Self::MarkUnread => Ok(EmailMutation::Keyword { keyword: "$seen", set: false }),
            Self::Flag => Ok(EmailMutation::Keyword { keyword: "$flagged", set: true }),
            Self::Unflag => Ok(EmailMutation::Keyword { keyword: "$flagged", set: false }),
            Self::Archive => {
                let mailbox_id = db::get_mailbox_by_role_or_name(conn, "archive", "Archive")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .ok_or(StatusCode::NOT_FOUND)?;
                Ok(EmailMutation::MoveTo { mailbox_id })
            }
            Self::Trash => {
                let mailbox_id = db::get_mailbox_by_role_or_name(conn, "trash", "Deleted Items")
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .ok_or(StatusCode::NOT_FOUND)?;
                Ok(EmailMutation::MoveTo { mailbox_id })
            }
            Self::Delete => Ok(EmailMutation::Destroy),
            Self::Move { mailbox_id } => Ok(EmailMutation::MoveTo { mailbox_id }),
        }
    }
}

pub async fn post_email_action(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(action): Json<EmailActionRequest>,
) -> Result<Json<EmailResponse>, StatusCode> {
    let client = state
        .jmap_client
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Resolve the action to a concrete mutation (may need DB for mailbox lookups).
    let mutation = {
        let state = state.clone();
        let action = action;
        tokio::task::spawn_blocking(move || {
            let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            action.into_mutation(&conn)
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??
    };

    // Apply JMAP mutation (remote).
    match &mutation {
        EmailMutation::Keyword { keyword, set } => {
            jmap::set_email_keyword(client, &id, keyword, *set)
                .await
                .map_err(|e| {
                    eprintln!("JMAP error: {e}");
                    StatusCode::BAD_GATEWAY
                })?;
        }
        EmailMutation::MoveTo { mailbox_id } => {
            jmap::move_email_to_mailbox(client, &id, mailbox_id)
                .await
                .map_err(|e| {
                    eprintln!("JMAP error: {e}");
                    StatusCode::BAD_GATEWAY
                })?;
        }
        EmailMutation::Destroy => {
            jmap::destroy_email(client, &id)
                .await
                .map_err(|e| {
                    eprintln!("JMAP error: {e}");
                    StatusCode::BAD_GATEWAY
                })?;
        }
    }

    // Apply local DB mutation and return updated email.
    let state = state.clone();
    let id = id.clone();
    tokio::task::spawn_blocking(move || {
        let conn = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        match mutation {
            EmailMutation::Keyword { keyword, set } => {
                db::update_email_keyword(&conn, &id, keyword, set)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            EmailMutation::MoveTo { mailbox_id } => {
                db::move_email_to_mailbox(&conn, &id, &mailbox_id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            EmailMutation::Destroy => {
                db::delete_email_record(&conn, &id)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                // Return empty response for delete — email no longer exists.
                return Err(StatusCode::NO_CONTENT);
            }
        }

        match db::get_email(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
            Some(row) => Ok(Json(email_row_to_response(row))),
            None => Err(StatusCode::NOT_FOUND),
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

// --- Compose & Send ---

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendEmailRequest {
    pub to: Vec<RecipientRequest>,
    #[serde(default)]
    pub cc: Vec<RecipientRequest>,
    #[serde(default)]
    pub bcc: Vec<RecipientRequest>,
    pub subject: String,
    pub body: String, // Markdown
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Deserialize)]
pub struct RecipientRequest {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Deserialize)]
pub struct PreviewRequest {
    pub body: String,
}

#[derive(Serialize)]
pub struct PreviewResponse {
    pub html: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityResponse {
    pub id: String,
    pub name: String,
    pub email: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyDataResponse {
    pub to: Vec<RecipientResponse>,
    pub cc: Vec<RecipientResponse>,
    pub reply_subject: String,
    pub forward_subject: String,
    pub quoted_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Serialize)]
pub struct RecipientResponse {
    pub name: Option<String>,
    pub email: String,
}

pub async fn preview_markdown(Json(req): Json<PreviewRequest>) -> Json<PreviewResponse> {
    Json(PreviewResponse {
        html: compose::render_markdown(&req.body),
    })
}

pub async fn get_identity(
    State(state): State<Arc<AppState>>,
) -> Result<Json<IdentityResponse>, StatusCode> {
    let client = state
        .jmap_client
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let data = compose::get_identity(client).await.map_err(|e| {
        eprintln!("Failed to get identity: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    Ok(Json(IdentityResponse {
        id: data.id,
        name: data.name,
        email: data.email,
    }))
}

pub async fn send_email(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendEmailRequest>,
) -> Result<StatusCode, StatusCode> {
    let client = state
        .jmap_client
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Look up the sender identity
    let identity = compose::get_identity(client).await.map_err(|e| {
        eprintln!("Failed to get identity: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    let to: Vec<(Option<String>, String)> = req
        .to
        .into_iter()
        .map(|r| (r.name, r.email))
        .collect();
    let cc: Vec<(Option<String>, String)> = req
        .cc
        .into_iter()
        .map(|r| (r.name, r.email))
        .collect();
    let bcc: Vec<(Option<String>, String)> = req
        .bcc
        .into_iter()
        .map(|r| (r.name, r.email))
        .collect();

    compose::send_email(
        client,
        &identity.id,
        &identity.name,
        &identity.email,
        &to,
        &cc,
        &bcc,
        &req.subject,
        &req.body,
        req.in_reply_to.as_deref(),
        req.references.as_deref(),
    )
    .await
    .map_err(|e| {
        eprintln!("Failed to send email: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    Ok(StatusCode::OK)
}

pub async fn get_reply_data(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ReplyDataResponse>, StatusCode> {
    let client = state
        .jmap_client
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let data = compose::get_reply_data(client, &id).await.map_err(|e| {
        eprintln!("Failed to get reply data: {e}");
        StatusCode::BAD_GATEWAY
    })?;

    // For simple reply: to = [original from], cc = []
    // The frontend can decide between reply and reply-all.
    let to: Vec<RecipientResponse> = data
        .from
        .into_iter()
        .map(|(name, email)| RecipientResponse { name, email })
        .collect();

    let cc: Vec<RecipientResponse> = data
        .cc_all
        .into_iter()
        .chain(data.to_all.into_iter())
        .map(|(name, email)| RecipientResponse { name, email })
        .collect();

    Ok(Json(ReplyDataResponse {
        to,
        cc,
        reply_subject: data.reply_subject,
        forward_subject: data.forward_subject,
        quoted_body: data.quoted_body,
        in_reply_to: data.in_reply_to,
        references: data.references,
    }))
}
