use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use crate::db;
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

pub async fn list_mailboxes(State(state): State<Arc<AppState>>) -> Json<Vec<MailboxResponse>> {
    let conn = state.db.lock().expect("db lock poisoned");
    let rows = db::get_mailboxes(&conn);
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
    Json(response)
}

pub async fn list_emails_in_mailbox(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Vec<EmailResponse>> {
    let conn = state.db.lock().expect("db lock poisoned");
    let rows = db::get_emails_in_mailbox(&conn, &id);
    Json(rows.into_iter().map(email_row_to_response).collect())
}

pub async fn list_all_emails(State(state): State<Arc<AppState>>) -> Json<Vec<EmailResponse>> {
    let conn = state.db.lock().expect("db lock poisoned");
    let rows = db::get_all_emails(&conn);
    Json(rows.into_iter().map(email_row_to_response).collect())
}

pub async fn get_email(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<EmailResponse>, StatusCode> {
    let conn = state.db.lock().expect("db lock poisoned");
    match db::get_email(&conn, &id) {
        Some(row) => Ok(Json(email_row_to_response(row))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn get_thread(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Vec<EmailResponse>> {
    let conn = state.db.lock().expect("db lock poisoned");
    let rows = db::get_thread(&conn, &id);
    Json(rows.into_iter().map(email_row_to_response).collect())
}
