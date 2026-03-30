use jmap_client::client::Client;
use jmap_client::{email, mailbox};
use rusqlite::Connection;

use crate::db::DbError;

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

pub async fn sync_mailboxes(client: &Client, conn: &Connection) -> Result<(), SyncError> {
    // Query all mailbox IDs
    let mailbox_ids = client
        .mailbox_query(
            None::<mailbox::query::Filter>,
            None::<Vec<_>>,
        )
        .await?
        .take_ids();

    if mailbox_ids.is_empty() {
        conn.execute("DELETE FROM mailbox", [])?;
        return Ok(());
    }

    // Fetch mailbox properties in bulk via request builder
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
    let mailbox_data = request
        .send_get_mailbox()
        .await?
        .take_list();

    // Clear and re-insert
    conn.execute("DELETE FROM mailbox", [])?;
    for mb in mailbox_data {
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
    }

    Ok(())
}

pub async fn sync_emails(client: &Client, conn: &Connection) -> Result<(), SyncError> {
    // Query email IDs sorted by receivedAt descending
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
        return Ok(());
    }

    println!("  Fetching {} emails in batches...", email_ids.len());

    // Clear tables before re-inserting
    conn.execute("DELETE FROM email", [])?;
    conn.execute("DELETE FROM email_mailbox", [])?;
    conn.execute("DELETE FROM email_keyword", [])?;

    // Fetch in batches of 100 (well within maxObjectsInGet: 500)
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
        let emails = request
            .send_get_email()
            .await?
            .take_list();

        println!("  Batch {}: {} emails fetched", i + 1, emails.len());
        insert_emails(conn, &emails)?;
    }

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
