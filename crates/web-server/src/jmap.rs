use jmap_client::client::Client;
use jmap_client::core::error::MethodErrorType;
use jmap_client::{email, mailbox};
use rusqlite::Connection;

use crate::db::{self, DbError};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("JMAP error: {0}")]
    Jmap(#[from] jmap_client::Error),
    #[error("Database error: {0}")]
    Database(#[from] DbError),
}

impl From<rusqlite::Error> for SyncError {
    fn from(err: rusqlite::Error) -> Self {
        SyncError::Database(DbError::from(err))
    }
}

pub async fn connect(url: &str, username: &str, password: &str) -> Result<Client, jmap_client::Error> {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
        .unwrap_or_default();

    Client::new()
        .credentials((username, password))
        .follow_redirects([host])
        .connect(url)
        .await
}

/// Returns true if delta sync was used (state existed).
pub async fn sync_mailboxes(client: &Client, conn: &Connection) -> Result<bool, SyncError> {
    let existing_state = db::get_sync_state(conn, "mailbox")?;

    match existing_state {
        Some(state) => {
            match delta_sync_mailboxes(client, conn, &state).await {
                Ok(()) => Ok(true),
                Err(e) if is_cannot_calculate_changes(&e) => {
                    eprintln!("  Mailbox delta sync failed (cannotCalculateChanges), falling back to full sync");
                    full_sync_mailboxes(client, conn).await?;
                    Ok(false)
                }
                Err(e) => Err(e),
            }
        }
        None => {
            full_sync_mailboxes(client, conn).await?;
            Ok(false)
        }
    }
}

/// Returns true if delta sync was used (state existed).
pub async fn sync_emails(client: &Client, conn: &Connection) -> Result<bool, SyncError> {
    let existing_state = db::get_sync_state(conn, "email")?;

    match existing_state {
        Some(state) => {
            match delta_sync_emails(client, conn, &state).await {
                Ok(()) => Ok(true),
                Err(e) if is_cannot_calculate_changes(&e) => {
                    eprintln!("  Email delta sync failed (cannotCalculateChanges), falling back to full sync");
                    full_sync_emails(client, conn).await?;
                    Ok(false)
                }
                Err(e) => Err(e),
            }
        }
        None => {
            full_sync_emails(client, conn).await?;
            Ok(false)
        }
    }
}

fn is_cannot_calculate_changes(err: &SyncError) -> bool {
    match err {
        SyncError::Jmap(jmap_client::Error::Method(me)) => {
            me.p_type == MethodErrorType::CannotCalculateChanges
        }
        _ => false,
    }
}

// --- Mailbox delta sync ---

async fn delta_sync_mailboxes(client: &Client, conn: &Connection, since_state: &str) -> Result<(), SyncError> {
    let state_preview = &since_state[..8.min(since_state.len())];
    println!("  Mailbox delta sync from state {state_preview}...");

    // Loop to handle has_more_changes
    let mut current_state = since_state.to_string();
    let mut total_created = 0usize;
    let mut total_updated = 0usize;
    let mut total_destroyed = 0usize;

    loop {
        let changes = client.mailbox_changes(&current_state, 500).await?;

        let created = changes.created();
        let updated = changes.updated();
        let destroyed = changes.destroyed();
        let has_more = changes.has_more_changes();

        total_created += created.len();
        total_updated += updated.len();
        total_destroyed += destroyed.len();

        // Delete destroyed mailboxes
        for id in destroyed {
            conn.execute("DELETE FROM mailbox WHERE id = ?1", rusqlite::params![id])?;
        }

        // Fetch and upsert created + updated
        let fetch_ids: Vec<&str> = created.iter().chain(updated.iter()).map(String::as_str).collect();
        if !fetch_ids.is_empty() {
            let mut request = client.build();
            request
                .get_mailbox()
                .ids(fetch_ids)
                .properties([
                    mailbox::Property::Id,
                    mailbox::Property::Name,
                    mailbox::Property::ParentId,
                    mailbox::Property::Role,
                    mailbox::Property::SortOrder,
                    mailbox::Property::TotalEmails,
                    mailbox::Property::UnreadEmails,
                ]);
            let mailbox_data = request.send_get_mailbox().await?.take_list();
            for mb in mailbox_data {
                insert_mailbox(conn, &mb)?;
            }
        }

        current_state = changes.new_state().to_string();

        if !has_more {
            break;
        }
    }

    println!(
        "  Mailbox changes: {} created, {} updated, {} destroyed",
        total_created, total_updated, total_destroyed
    );
    db::save_sync_state(conn, "mailbox", &current_state)?;
    Ok(())
}

// --- Email delta sync ---

async fn delta_sync_emails(client: &Client, conn: &Connection, since_state: &str) -> Result<(), SyncError> {
    let state_preview = &since_state[..8.min(since_state.len())];
    println!("  Email delta sync from state {state_preview}...");

    let mut current_state = since_state.to_string();
    let mut total_created = 0usize;
    let mut total_updated = 0usize;
    let mut total_destroyed = 0usize;

    loop {
        let changes = client.email_changes(&current_state, Some(500)).await?;

        let created = changes.created();
        let updated = changes.updated();
        let destroyed = changes.destroyed();
        let has_more = changes.has_more_changes();

        total_created += created.len();
        total_updated += updated.len();
        total_destroyed += destroyed.len();

        // Delete destroyed emails (with junction tables)
        if !destroyed.is_empty() {
            db::delete_emails_by_ids(conn, destroyed)?;
        }

        // Fetch and upsert created + updated in batches
        let fetch_ids: Vec<String> = created.iter().chain(updated.iter()).cloned().collect();
        if !fetch_ids.is_empty() {
            for chunk in fetch_ids.chunks(100) {
                let mut request = client.build();
                request
                    .get_email()
                    .ids(chunk.iter().map(String::as_str))
                    .properties([
                        email::Property::Id,
                        email::Property::ThreadId,
                        email::Property::MailboxIds,
                        email::Property::Keywords,
                        email::Property::Subject,
                        email::Property::From,
                        email::Property::Preview,
                        email::Property::ReceivedAt,
                        email::Property::HasAttachment,
                        email::Property::Size,
                    ]);
                let emails = request.send_get_email().await?.take_list();

                // For updated emails, clear old junction rows before re-inserting
                for id in chunk {
                    conn.execute("DELETE FROM email_mailbox WHERE email_id = ?1", rusqlite::params![id])?;
                    conn.execute("DELETE FROM email_keyword WHERE email_id = ?1", rusqlite::params![id])?;
                }

                insert_emails(conn, &emails)?;
            }
        }

        current_state = changes.new_state().to_string();

        if !has_more {
            break;
        }
    }

    println!(
        "  Email changes: {} created, {} updated, {} destroyed",
        total_created, total_updated, total_destroyed
    );
    db::save_sync_state(conn, "email", &current_state)?;
    Ok(())
}

// --- Full sync (first run or fallback) ---

async fn full_sync_mailboxes(client: &Client, conn: &Connection) -> Result<(), SyncError> {
    println!("  Full mailbox sync...");

    let mailbox_ids = client
        .mailbox_query(
            None::<mailbox::query::Filter>,
            None::<Vec<_>>,
        )
        .await?
        .take_ids();

    if mailbox_ids.is_empty() {
        conn.execute("DELETE FROM mailbox", [])?;
        db::save_sync_state(conn, "mailbox", "")?;
        return Ok(());
    }

    let mut request = client.build();
    request
        .get_mailbox()
        .ids(mailbox_ids.iter().map(String::as_str))
        .properties([
            mailbox::Property::Id,
            mailbox::Property::Name,
            mailbox::Property::ParentId,
            mailbox::Property::Role,
            mailbox::Property::SortOrder,
            mailbox::Property::TotalEmails,
            mailbox::Property::UnreadEmails,
        ]);
    let mut response = request.send_get_mailbox().await?;
    let state = response.take_state();
    let mailbox_data = response.take_list();

    // Clear and re-insert
    conn.execute("DELETE FROM mailbox", [])?;
    for mb in &mailbox_data {
        insert_mailbox(conn, mb)?;
    }

    println!("  Synced {} mailboxes", mailbox_data.len());
    db::save_sync_state(conn, "mailbox", &state)?;
    Ok(())
}

async fn full_sync_emails(client: &Client, conn: &Connection) -> Result<(), SyncError> {
    println!("  Full email sync...");

    let email_ids = client
        .email_query(
            None::<email::query::Filter>,
            [email::query::Comparator::received_at().descending()].into(),
        )
        .await?
        .take_ids();

    if email_ids.is_empty() {
        conn.execute("DELETE FROM email", [])?;
        conn.execute("DELETE FROM email_mailbox", [])?;
        conn.execute("DELETE FROM email_keyword", [])?;
        db::save_sync_state(conn, "email", "")?;
        return Ok(());
    }

    println!("  Fetching {} emails in batches...", email_ids.len());

    // Clear tables before re-inserting
    conn.execute("DELETE FROM email", [])?;
    conn.execute("DELETE FROM email_mailbox", [])?;
    conn.execute("DELETE FROM email_keyword", [])?;

    // Capture state from the first batch response
    let mut saved_state: Option<String> = None;
    let batch_size = 100;

    for (i, chunk) in email_ids.chunks(batch_size).enumerate() {
        let mut request = client.build();
        request
            .get_email()
            .ids(chunk.iter().map(String::as_str))
            .properties([
                email::Property::Id,
                email::Property::ThreadId,
                email::Property::MailboxIds,
                email::Property::Keywords,
                email::Property::Subject,
                email::Property::From,
                email::Property::Preview,
                email::Property::ReceivedAt,
                email::Property::HasAttachment,
                email::Property::Size,
            ]);
        let mut response = request.send_get_email().await?;

        if saved_state.is_none() {
            saved_state = Some(response.take_state());
        }

        let emails = response.take_list();
        println!("  Batch {}: {} emails fetched", i + 1, emails.len());
        insert_emails(conn, &emails)?;
    }

    if let Some(state) = saved_state {
        db::save_sync_state(conn, "email", &state)?;
    }

    Ok(())
}

// --- Email mutation helpers ---

/// Set or unset a keyword ($seen, $flagged, etc.)
pub async fn set_email_keyword(
    client: &Client,
    email_id: &str,
    keyword: &str,
    set: bool,
) -> Result<(), SyncError> {
    client.email_set_keyword(email_id, keyword, set).await?;
    Ok(())
}

/// Move email to a single mailbox (replaces all current mailbox memberships).
pub async fn move_email_to_mailbox(
    client: &Client,
    email_id: &str,
    mailbox_id: &str,
) -> Result<(), SyncError> {
    client
        .email_set_mailboxes(email_id, [mailbox_id])
        .await?;
    Ok(())
}

/// Permanently destroy an email.
pub async fn destroy_email(client: &Client, email_id: &str) -> Result<(), SyncError> {
    client.email_destroy(email_id).await?;
    Ok(())
}

// --- Shared helpers ---

fn insert_mailbox(conn: &Connection, mb: &jmap_client::mailbox::Mailbox) -> Result<(), SyncError> {
    let id = mb.id().unwrap_or_default();
    let name = mb.name().unwrap_or("(unnamed)");
    let parent_id = mb.parent_id();
    let role = mb.role();
    let sort_order = mb.sort_order();
    let total_emails = mb.total_emails();
    let unread_emails = mb.unread_emails();

    let role_str: Option<String> = match role {
        mailbox::Role::Inbox => Some("inbox".into()),
        mailbox::Role::Drafts => Some("drafts".into()),
        mailbox::Role::Sent => Some("sent".into()),
        mailbox::Role::Trash => Some("trash".into()),
        mailbox::Role::Junk => Some("junk".into()),
        mailbox::Role::Archive => Some("archive".into()),
        mailbox::Role::None => None,
        other => Some(format!("{:?}", other).to_lowercase()),
    };

    conn.execute(
        "INSERT OR REPLACE INTO mailbox (id, name, parent_id, role, sort_order, total_emails, unread_emails) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![id, name, parent_id, role_str, sort_order as i64, total_emails as i64, unread_emails as i64],
    )?;

    Ok(())
}

pub async fn fetch_email_body(
    client: &Client,
    email_id: &str,
) -> Result<(Option<String>, Option<String>), SyncError> {
    let mut request = client.build();
    let get_request = request.get_email().ids([email_id]).properties([
        email::Property::Id,
        email::Property::BodyValues,
        email::Property::HtmlBody,
        email::Property::TextBody,
    ]);
    get_request
        .arguments()
        .body_properties([email::BodyProperty::PartId, email::BodyProperty::Type])
        .fetch_all_body_values(true);

    let mut emails = request.send_get_email().await?.take_list();
    let email = emails
        .pop()
        .ok_or_else(|| SyncError::Jmap(jmap_client::Error::Internal("email not found".into())))?;

    let html = email.html_body().and_then(|parts| {
        parts
            .first()
            .and_then(|part| part.part_id())
            .and_then(|part_id| email.body_value(part_id))
            .map(|v| v.value().to_string())
    });

    let text = email.text_body().and_then(|parts| {
        parts
            .first()
            .and_then(|part| part.part_id())
            .and_then(|part_id| email.body_value(part_id))
            .map(|v| v.value().to_string())
    });

    Ok((html, text))
}

fn insert_emails(conn: &Connection, emails: &[jmap_client::email::Email]) -> Result<(), SyncError> {
    for em in emails {
        let id = em.id().unwrap_or_default();
        let thread_id = em.thread_id().unwrap_or_default();
        let subject = em.subject();
        let preview = em.preview();
        let received_at = em.received_at().unwrap_or(0);
        let has_attachment = em.has_attachment();
        let size = em.size();

        let (from_name, from_email) = em
            .from()
            .and_then(|addrs| addrs.first())
            .map(|addr| (addr.name(), Some(addr.email())))
            .unwrap_or((None, None));

        let keywords = em.keywords();
        let is_read = keywords.contains(&"$seen");
        let is_flagged = keywords.contains(&"$flagged");

        conn.execute(
            "INSERT OR REPLACE INTO email (id, thread_id, subject, from_name, from_email, preview, received_at, has_attachment, size, is_read, is_flagged) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment as i32, size as i64,
                is_read as i32, is_flagged as i32,
            ],
        )?;

        for mailbox_id in em.mailbox_ids() {
            conn.execute(
                "INSERT OR REPLACE INTO email_mailbox (email_id, mailbox_id) VALUES (?1, ?2)",
                rusqlite::params![id, mailbox_id],
            )?;
        }

        for keyword in &keywords {
            conn.execute(
                "INSERT OR REPLACE INTO email_keyword (email_id, keyword) VALUES (?1, ?2)",
                rusqlite::params![id, keyword],
            )?;
        }
    }

    Ok(())
}
