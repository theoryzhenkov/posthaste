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

// --- Intermediate data types for fetch/apply split ---

pub struct MailboxData {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub role: Option<String>,
    pub sort_order: i64,
    pub total_emails: i64,
    pub unread_emails: i64,
}

pub struct MailboxSyncResult {
    pub clear_all: bool,
    pub upsert: Vec<MailboxData>,
    pub delete_ids: Vec<String>,
    pub new_state: String,
    pub created_count: usize,
    pub updated_count: usize,
    pub destroyed_count: usize,
}

pub struct EmailSyncResult {
    pub clear_all: bool,
    pub upsert: Vec<EmailData>,
    pub delete_ids: Vec<String>,
    pub new_state: String,
    pub created_count: usize,
    pub updated_count: usize,
    pub destroyed_count: usize,
}

pub struct EmailData {
    pub id: String,
    pub thread_id: String,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: i64,
    pub has_attachment: bool,
    pub size: i64,
    pub is_read: bool,
    pub is_flagged: bool,
    pub mailbox_ids: Vec<String>,
    pub keywords: Vec<String>,
}

// --- Connection ---

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

// --- Public combined wrappers (used at startup) ---

/// Returns true if delta sync was used (state existed).
pub async fn sync_mailboxes(client: &Client, conn: &Connection) -> Result<bool, SyncError> {
    let since_state = db::get_sync_state(conn, "mailbox")?;
    let result = fetch_mailbox_sync(client, since_state.as_deref()).await?;
    let was_delta = !result.clear_all;
    let count = result.upsert.len();
    apply_mailbox_sync(conn, &result)?;
    if was_delta {
        println!(
            "  Mailbox changes: {} created, {} updated, {} destroyed",
            result.created_count, result.updated_count, result.destroyed_count,
        );
    } else {
        println!("  Synced {} mailboxes", count);
    }
    Ok(was_delta)
}

/// Returns true if delta sync was used (state existed).
pub async fn sync_emails(client: &Client, conn: &Connection) -> Result<bool, SyncError> {
    let since_state = db::get_sync_state(conn, "email")?;
    let result = fetch_email_sync(client, since_state.as_deref()).await?;
    let was_delta = !result.clear_all;
    let count = result.upsert.len();
    apply_email_sync(conn, &result)?;
    if was_delta {
        println!(
            "  Email changes: {} created, {} updated, {} destroyed",
            result.created_count, result.updated_count, result.destroyed_count,
        );
    } else {
        println!("  Synced {} emails", count);
    }
    Ok(was_delta)
}

// --- Fetch functions (async, no DB) ---

pub async fn fetch_mailbox_sync(
    client: &Client,
    since_state: Option<&str>,
) -> Result<MailboxSyncResult, SyncError> {
    match since_state {
        Some(state) => match fetch_mailbox_delta(client, state).await {
            Ok(result) => Ok(result),
            Err(e) if is_cannot_calculate_changes(&e) => {
                eprintln!("  Mailbox delta sync failed (cannotCalculateChanges), falling back to full sync");
                fetch_mailbox_full(client).await
            }
            Err(e) => Err(e),
        },
        None => fetch_mailbox_full(client).await,
    }
}

pub async fn fetch_email_sync(
    client: &Client,
    since_state: Option<&str>,
) -> Result<EmailSyncResult, SyncError> {
    match since_state {
        Some(state) => match fetch_email_delta(client, state).await {
            Ok(result) => Ok(result),
            Err(e) if is_cannot_calculate_changes(&e) => {
                eprintln!("  Email delta sync failed (cannotCalculateChanges), falling back to full sync");
                fetch_email_full(client).await
            }
            Err(e) => Err(e),
        },
        None => fetch_email_full(client).await,
    }
}

// --- Apply functions (sync, DB only) ---

pub fn apply_mailbox_sync(conn: &Connection, result: &MailboxSyncResult) -> Result<(), SyncError> {
    if result.clear_all {
        conn.execute("DELETE FROM mailbox", [])?;
    }

    for id in &result.delete_ids {
        conn.execute(
            "DELETE FROM mailbox WHERE id = ?1",
            rusqlite::params![id],
        )?;
    }

    for mb in &result.upsert {
        conn.execute(
            "INSERT OR REPLACE INTO mailbox (id, name, parent_id, role, sort_order, total_emails, unread_emails) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![mb.id, mb.name, mb.parent_id, mb.role, mb.sort_order, mb.total_emails, mb.unread_emails],
        )?;
    }

    db::save_sync_state(conn, "mailbox", &result.new_state)?;
    Ok(())
}

pub fn apply_email_sync(conn: &Connection, result: &EmailSyncResult) -> Result<(), SyncError> {
    if result.clear_all {
        conn.execute("DELETE FROM email", [])?;
        conn.execute("DELETE FROM email_mailbox", [])?;
        conn.execute("DELETE FROM email_keyword", [])?;
    }

    if !result.delete_ids.is_empty() {
        db::delete_emails_by_ids(conn, &result.delete_ids)?;
    }

    // For delta updates, clear old junction rows before re-inserting
    if !result.clear_all {
        for em in &result.upsert {
            conn.execute(
                "DELETE FROM email_mailbox WHERE email_id = ?1",
                rusqlite::params![em.id],
            )?;
            conn.execute(
                "DELETE FROM email_keyword WHERE email_id = ?1",
                rusqlite::params![em.id],
            )?;
        }
    }

    for em in &result.upsert {
        conn.execute(
            "INSERT OR REPLACE INTO email (id, thread_id, subject, from_name, from_email, preview, received_at, has_attachment, size, is_read, is_flagged) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                em.id, em.thread_id, em.subject, em.from_name, em.from_email, em.preview,
                em.received_at, em.has_attachment as i32, em.size,
                em.is_read as i32, em.is_flagged as i32,
            ],
        )?;

        for mailbox_id in &em.mailbox_ids {
            conn.execute(
                "INSERT OR REPLACE INTO email_mailbox (email_id, mailbox_id) VALUES (?1, ?2)",
                rusqlite::params![em.id, mailbox_id],
            )?;
        }

        for keyword in &em.keywords {
            conn.execute(
                "INSERT OR REPLACE INTO email_keyword (email_id, keyword) VALUES (?1, ?2)",
                rusqlite::params![em.id, keyword],
            )?;
        }
    }

    db::save_sync_state(conn, "email", &result.new_state)?;
    Ok(())
}

// --- Delta fetch (async, no DB) ---

async fn fetch_mailbox_delta(
    client: &Client,
    since_state: &str,
) -> Result<MailboxSyncResult, SyncError> {
    let mut current_state = since_state.to_string();
    let mut all_upsert = Vec::new();
    let mut all_delete_ids = Vec::new();
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

        all_delete_ids.extend(destroyed.iter().cloned());

        // Fetch created + updated mailbox data
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
            let mailbox_list = request.send_get_mailbox().await?.take_list();
            for mb in mailbox_list {
                all_upsert.push(jmap_mailbox_to_data(&mb));
            }
        }

        current_state = changes.new_state().to_string();

        if !has_more {
            break;
        }
    }

    Ok(MailboxSyncResult {
        clear_all: false,
        upsert: all_upsert,
        delete_ids: all_delete_ids,
        new_state: current_state,
        created_count: total_created,
        updated_count: total_updated,
        destroyed_count: total_destroyed,
    })
}

async fn fetch_email_delta(
    client: &Client,
    since_state: &str,
) -> Result<EmailSyncResult, SyncError> {
    let mut current_state = since_state.to_string();
    let mut all_upsert = Vec::new();
    let mut all_delete_ids = Vec::new();
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

        all_delete_ids.extend(destroyed.iter().cloned());

        // Fetch created + updated emails in batches
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
                for em in &emails {
                    all_upsert.push(jmap_email_to_data(em));
                }
            }
        }

        current_state = changes.new_state().to_string();

        if !has_more {
            break;
        }
    }

    Ok(EmailSyncResult {
        clear_all: false,
        upsert: all_upsert,
        delete_ids: all_delete_ids,
        new_state: current_state,
        created_count: total_created,
        updated_count: total_updated,
        destroyed_count: total_destroyed,
    })
}

// --- Full fetch (async, no DB) ---

async fn fetch_mailbox_full(client: &Client) -> Result<MailboxSyncResult, SyncError> {
    let mailbox_ids = client
        .mailbox_query(None::<mailbox::query::Filter>, None::<Vec<_>>)
        .await?
        .take_ids();

    if mailbox_ids.is_empty() {
        return Ok(MailboxSyncResult {
            clear_all: true,
            upsert: Vec::new(),
            delete_ids: Vec::new(),
            new_state: String::new(),
            created_count: 0,
            updated_count: 0,
            destroyed_count: 0,
        });
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
    let mailbox_list = response.take_list();

    let upsert: Vec<MailboxData> = mailbox_list.iter().map(jmap_mailbox_to_data).collect();

    Ok(MailboxSyncResult {
        clear_all: true,
        upsert,
        delete_ids: Vec::new(),
        new_state: state,
        created_count: 0,
        updated_count: 0,
        destroyed_count: 0,
    })
}

async fn fetch_email_full(client: &Client) -> Result<EmailSyncResult, SyncError> {
    let email_ids = client
        .email_query(
            None::<email::query::Filter>,
            [email::query::Comparator::received_at().descending()].into(),
        )
        .await?
        .take_ids();

    if email_ids.is_empty() {
        return Ok(EmailSyncResult {
            clear_all: true,
            upsert: Vec::new(),
            delete_ids: Vec::new(),
            new_state: String::new(),
            created_count: 0,
            updated_count: 0,
            destroyed_count: 0,
        });
    }

    let mut all_upsert = Vec::new();
    let mut saved_state: Option<String> = None;

    for chunk in email_ids.chunks(100) {
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
        for em in &emails {
            all_upsert.push(jmap_email_to_data(em));
        }
    }

    Ok(EmailSyncResult {
        clear_all: true,
        upsert: all_upsert,
        delete_ids: Vec::new(),
        new_state: saved_state.unwrap_or_default(),
        created_count: 0,
        updated_count: 0,
        destroyed_count: 0,
    })
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

// --- Shared helpers ---

fn is_cannot_calculate_changes(err: &SyncError) -> bool {
    match err {
        SyncError::Jmap(jmap_client::Error::Method(me)) => {
            me.p_type == MethodErrorType::CannotCalculateChanges
        }
        _ => false,
    }
}

fn jmap_mailbox_to_data(mb: &jmap_client::mailbox::Mailbox) -> MailboxData {
    let role_str: Option<String> = match mb.role() {
        mailbox::Role::Inbox => Some("inbox".into()),
        mailbox::Role::Drafts => Some("drafts".into()),
        mailbox::Role::Sent => Some("sent".into()),
        mailbox::Role::Trash => Some("trash".into()),
        mailbox::Role::Junk => Some("junk".into()),
        mailbox::Role::Archive => Some("archive".into()),
        mailbox::Role::None => None,
        other => Some(format!("{:?}", other).to_lowercase()),
    };

    MailboxData {
        id: mb.id().unwrap_or_default().to_string(),
        name: mb.name().unwrap_or("(unnamed)").to_string(),
        parent_id: mb.parent_id().map(String::from),
        role: role_str,
        sort_order: mb.sort_order() as i64,
        total_emails: mb.total_emails() as i64,
        unread_emails: mb.unread_emails() as i64,
    }
}

fn jmap_email_to_data(em: &jmap_client::email::Email) -> EmailData {
    let keywords = em.keywords();
    let is_read = keywords.contains(&"$seen");
    let is_flagged = keywords.contains(&"$flagged");

    let (from_name, from_email) = em
        .from()
        .and_then(|addrs| addrs.first())
        .map(|addr| (addr.name().map(String::from), Some(addr.email().to_string())))
        .unwrap_or((None, None));

    EmailData {
        id: em.id().unwrap_or_default().to_string(),
        thread_id: em.thread_id().unwrap_or_default().to_string(),
        subject: em.subject().map(String::from),
        from_name,
        from_email,
        preview: em.preview().map(String::from),
        received_at: em.received_at().unwrap_or(0),
        has_attachment: em.has_attachment(),
        size: em.size() as i64,
        is_read,
        is_flagged,
        mailbox_ids: em.mailbox_ids().into_iter().map(String::from).collect(),
        keywords: keywords.into_iter().map(String::from).collect(),
    }
}
