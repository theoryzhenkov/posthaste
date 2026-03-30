use std::time::{SystemTime, UNIX_EPOCH};

use mail_engine::MailEngine;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct MailboxRow {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub role: Option<String>,
    pub sort_order: i64,
    pub total_emails: i64,
    pub unread_emails: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailRow {
    pub id: String,
    pub thread_id: String,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: i64,
    pub has_attachment: bool,
    pub is_read: bool,
    pub is_flagged: bool,
    pub size: i64,
    pub mailbox_ids: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmailBodyRow {
    pub email_id: String,
    pub html: Option<String>,
    pub text_body: Option<String>,
}

pub fn init_db(path: &str) -> Result<Connection, DbError> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "wal")?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS mailbox (
            id TEXT NOT NULL,
            account_id TEXT NOT NULL DEFAULT 'mock-account-1',
            name TEXT NOT NULL,
            parent_id TEXT,
            role TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            total_emails INTEGER NOT NULL DEFAULT 0,
            unread_emails INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (account_id, id)
        );

        CREATE TABLE IF NOT EXISTS email (
            id TEXT NOT NULL,
            account_id TEXT NOT NULL DEFAULT 'mock-account-1',
            thread_id TEXT NOT NULL,
            subject TEXT,
            from_name TEXT,
            from_email TEXT,
            preview TEXT,
            received_at INTEGER NOT NULL,
            has_attachment INTEGER NOT NULL DEFAULT 0,
            size INTEGER NOT NULL DEFAULT 0,
            is_read INTEGER NOT NULL DEFAULT 1,
            is_flagged INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (account_id, id)
        );

        CREATE TABLE IF NOT EXISTS email_mailbox (
            account_id TEXT NOT NULL DEFAULT 'mock-account-1',
            email_id TEXT NOT NULL,
            mailbox_id TEXT NOT NULL,
            PRIMARY KEY (account_id, email_id, mailbox_id)
        );

        CREATE TABLE IF NOT EXISTS email_keyword (
            account_id TEXT NOT NULL DEFAULT 'mock-account-1',
            email_id TEXT NOT NULL,
            keyword TEXT NOT NULL,
            PRIMARY KEY (account_id, email_id, keyword)
        );

        CREATE TABLE IF NOT EXISTS email_body (
            email_id TEXT PRIMARY KEY,
            account_id TEXT NOT NULL DEFAULT 'mock-account-1',
            html TEXT,
            text_body TEXT,
            fetched_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sync_state (
            type_name TEXT PRIMARY KEY,
            state TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );
        ",
    )?;

    Ok(conn)
}

pub fn get_sync_state(conn: &Connection, type_name: &str) -> Result<Option<String>, DbError> {
    let mut stmt = conn.prepare("SELECT state FROM sync_state WHERE type_name = ?1")?;
    let mut rows = stmt.query_map(params![type_name], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn save_sync_state(conn: &Connection, type_name: &str, state: &str) -> Result<(), DbError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO sync_state (type_name, state, updated_at) VALUES (?1, ?2, ?3)",
        params![type_name, state, now],
    )?;
    Ok(())
}

pub fn delete_emails_by_ids(conn: &Connection, ids: &[String]) -> Result<(), DbError> {
    for id in ids {
        conn.execute("DELETE FROM email_keyword WHERE email_id = ?1", params![id])?;
        conn.execute("DELETE FROM email_mailbox WHERE email_id = ?1", params![id])?;
        conn.execute("DELETE FROM email_body WHERE email_id = ?1", params![id])?;
        conn.execute("DELETE FROM email WHERE id = ?1", params![id])?;
    }
    Ok(())
}

pub fn import_mock_data(conn: &Connection) {
    conn.execute("DELETE FROM email_keyword", []).expect("failed to clear email_keyword");
    conn.execute("DELETE FROM email_mailbox", []).expect("failed to clear email_mailbox");
    conn.execute("DELETE FROM email", []).expect("failed to clear email");
    conn.execute("DELETE FROM mailbox", []).expect("failed to clear mailbox");

    let engine = MailEngine::new();

    for mb in engine.get_mailboxes() {
        conn.execute(
            "INSERT INTO mailbox (id, name, parent_id, role, sort_order, total_emails, unread_emails)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                mb.id,
                mb.name,
                mb.parent_id,
                mb.role,
                mb.sort_order,
                mb.total_emails,
                mb.unread_emails,
            ],
        )
        .expect("failed to insert mailbox");
    }

    for email in engine.get_all_emails() {
        let is_read = email.keywords.contains(&"$seen".to_string());
        let is_flagged = email.keywords.contains(&"$flagged".to_string());

        conn.execute(
            "INSERT INTO email (id, thread_id, subject, from_name, from_email, preview, received_at, has_attachment, size, is_read, is_flagged)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                email.id,
                email.thread_id,
                email.subject,
                email.from_name,
                email.from_email,
                email.preview,
                email.received_at,
                email.has_attachment as i32,
                email.size as i64,
                is_read as i32,
                is_flagged as i32,
            ],
        )
        .expect("failed to insert email");

        for mailbox_id in &email.mailbox_ids {
            conn.execute(
                "INSERT INTO email_mailbox (email_id, mailbox_id) VALUES (?1, ?2)",
                params![email.id, mailbox_id],
            )
            .expect("failed to insert email_mailbox");
        }

        for keyword in &email.keywords {
            conn.execute(
                "INSERT INTO email_keyword (email_id, keyword) VALUES (?1, ?2)",
                params![email.id, keyword],
            )
            .expect("failed to insert email_keyword");
        }
    }
}

pub fn get_mailboxes(conn: &Connection) -> Result<Vec<MailboxRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, parent_id, role, sort_order, total_emails, unread_emails
         FROM mailbox ORDER BY sort_order",
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok(MailboxRow {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                role: row.get(3)?,
                sort_order: row.get(4)?,
                total_emails: row.get(5)?,
                unread_emails: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows)
}

fn query_emails(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<EmailRow>, DbError> {
    let mut stmt = conn.prepare(sql)?;

    let email_rows: Vec<EmailRow> = stmt
        .query_map(params, |row| {
            Ok(EmailRow {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                subject: row.get(2)?,
                from_name: row.get(3)?,
                from_email: row.get(4)?,
                preview: row.get(5)?,
                received_at: row.get(6)?,
                has_attachment: row.get::<_, i32>(7)? != 0,
                is_read: row.get::<_, i32>(8)? != 0,
                is_flagged: row.get::<_, i32>(9)? != 0,
                size: row.get(10)?,
                mailbox_ids: Vec::new(),
                keywords: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Prepare enrichment statements once outside the loop
    let mut mailbox_stmt = conn.prepare("SELECT mailbox_id FROM email_mailbox WHERE email_id = ?1")?;
    let mut keyword_stmt = conn.prepare("SELECT keyword FROM email_keyword WHERE email_id = ?1")?;

    let enriched = email_rows
        .into_iter()
        .map(|mut e| {
            e.mailbox_ids = mailbox_stmt
                .query_map(params![e.id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;

            e.keywords = keyword_stmt
                .query_map(params![e.id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;

            Ok(e)
        })
        .collect::<Result<Vec<_>, DbError>>()?;

    Ok(enriched)
}

pub fn get_emails_in_mailbox(conn: &Connection, mailbox_id: &str) -> Result<Vec<EmailRow>, DbError> {
    query_emails(
        conn,
        "SELECT e.id, e.thread_id, e.subject, e.from_name, e.from_email, e.preview,
                e.received_at, e.has_attachment, e.is_read, e.is_flagged, e.size
         FROM email e
         JOIN email_mailbox em ON em.email_id = e.id
         WHERE em.mailbox_id = ?1
         ORDER BY e.received_at DESC",
        &[&mailbox_id as &dyn rusqlite::ToSql],
    )
}

pub fn get_all_emails(conn: &Connection) -> Result<Vec<EmailRow>, DbError> {
    query_emails(
        conn,
        "SELECT id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment, is_read, is_flagged, size
         FROM email ORDER BY received_at DESC",
        &[],
    )
}

pub fn get_email(conn: &Connection, email_id: &str) -> Result<Option<EmailRow>, DbError> {
    let results = query_emails(
        conn,
        "SELECT id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment, is_read, is_flagged, size
         FROM email WHERE id = ?1",
        &[&email_id as &dyn rusqlite::ToSql],
    )?;
    Ok(results.into_iter().next())
}

pub fn get_thread(conn: &Connection, thread_id: &str) -> Result<Vec<EmailRow>, DbError> {
    query_emails(
        conn,
        "SELECT id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment, is_read, is_flagged, size
         FROM email WHERE thread_id = ?1 ORDER BY received_at ASC",
        &[&thread_id as &dyn rusqlite::ToSql],
    )
}

/// Get mailbox ID by role, falling back to name lookup.
pub fn get_mailbox_by_role_or_name(
    conn: &Connection,
    role: &str,
    name: &str,
) -> Result<Option<String>, DbError> {
    // Try role first
    let mut stmt = conn.prepare("SELECT id FROM mailbox WHERE role = ?1 LIMIT 1")?;
    let mut rows = stmt.query_map(params![role], |row| row.get::<_, String>(0))?;
    if let Some(row) = rows.next() {
        return Ok(Some(row?));
    }
    // Fall back to name
    let mut stmt = conn.prepare("SELECT id FROM mailbox WHERE name = ?1 LIMIT 1")?;
    let mut rows = stmt.query_map(params![name], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Update is_read/is_flagged column and email_keyword table.
pub fn update_email_keyword(
    conn: &Connection,
    email_id: &str,
    keyword: &str,
    set: bool,
) -> Result<(), DbError> {
    // Update denormalized column
    match keyword {
        "$seen" => {
            conn.execute(
                "UPDATE email SET is_read = ?1 WHERE id = ?2",
                params![set as i32, email_id],
            )?;
        }
        "$flagged" => {
            conn.execute(
                "UPDATE email SET is_flagged = ?1 WHERE id = ?2",
                params![set as i32, email_id],
            )?;
        }
        _ => {}
    }

    // Update keyword table
    if set {
        conn.execute(
            "INSERT OR IGNORE INTO email_keyword (email_id, keyword) VALUES (?1, ?2)",
            params![email_id, keyword],
        )?;
    } else {
        conn.execute(
            "DELETE FROM email_keyword WHERE email_id = ?1 AND keyword = ?2",
            params![email_id, keyword],
        )?;
    }

    Ok(())
}

/// Move email: remove from current mailboxes, add to target mailbox.
pub fn move_email_to_mailbox(
    conn: &Connection,
    email_id: &str,
    target_mailbox_id: &str,
) -> Result<(), DbError> {
    conn.execute(
        "DELETE FROM email_mailbox WHERE email_id = ?1",
        params![email_id],
    )?;
    conn.execute(
        "INSERT INTO email_mailbox (email_id, mailbox_id) VALUES (?1, ?2)",
        params![email_id, target_mailbox_id],
    )?;
    Ok(())
}

/// Delete email from all tables.
pub fn delete_email_record(conn: &Connection, email_id: &str) -> Result<(), DbError> {
    conn.execute("DELETE FROM email_keyword WHERE email_id = ?1", params![email_id])?;
    conn.execute("DELETE FROM email_mailbox WHERE email_id = ?1", params![email_id])?;
    conn.execute("DELETE FROM email_body WHERE email_id = ?1", params![email_id])?;
    conn.execute("DELETE FROM email WHERE id = ?1", params![email_id])?;
    Ok(())
}

pub fn get_email_body(conn: &Connection, email_id: &str) -> Result<Option<EmailBodyRow>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT email_id, html, text_body FROM email_body WHERE email_id = ?1",
    )?;

    let mut rows = stmt.query_map(params![email_id], |row| {
        Ok(EmailBodyRow {
            email_id: row.get(0)?,
            html: row.get(1)?,
            text_body: row.get(2)?,
        })
    })?;

    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn save_email_body(
    conn: &Connection,
    email_id: &str,
    html: Option<&str>,
    text_body: Option<&str>,
) -> Result<(), DbError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO email_body (email_id, html, text_body, fetched_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![email_id, html, text_body, now],
    )?;

    Ok(())
}
