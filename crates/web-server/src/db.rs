use mail_engine::MailEngine;
use rusqlite::{params, Connection, Result};
use serde::Serialize;

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

pub fn init_db(path: &str) -> Connection {
    let conn = Connection::open(path).expect("failed to open SQLite database");
    conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

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
        ",
    )
    .expect("failed to create tables");

    conn
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

pub fn get_mailboxes(conn: &Connection) -> Vec<MailboxRow> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, parent_id, role, sort_order, total_emails, unread_emails
             FROM mailbox ORDER BY sort_order",
        )
        .expect("failed to prepare mailbox query");

    stmt.query_map([], |row| {
        Ok(MailboxRow {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            role: row.get(3)?,
            sort_order: row.get(4)?,
            total_emails: row.get(5)?,
            unread_emails: row.get(6)?,
        })
    })
    .expect("failed to query mailboxes")
    .collect::<Result<Vec<_>>>()
    .expect("failed to collect mailboxes")
}

fn query_emails(conn: &Connection, sql: &str, params: &[&dyn rusqlite::ToSql]) -> Vec<EmailRow> {
    let mut stmt = conn.prepare(sql).expect("failed to prepare email query");

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
        })
        .expect("failed to query emails")
        .collect::<Result<Vec<_>>>()
        .expect("failed to collect emails");

    // Enrich with mailbox_ids and keywords
    email_rows
        .into_iter()
        .map(|mut e| {
            e.mailbox_ids = conn
                .prepare("SELECT mailbox_id FROM email_mailbox WHERE email_id = ?1")
                .expect("failed to prepare mailbox_ids query")
                .query_map(params![e.id], |row| row.get(0))
                .expect("failed to query mailbox_ids")
                .collect::<Result<Vec<String>>>()
                .expect("failed to collect mailbox_ids");

            e.keywords = conn
                .prepare("SELECT keyword FROM email_keyword WHERE email_id = ?1")
                .expect("failed to prepare keywords query")
                .query_map(params![e.id], |row| row.get(0))
                .expect("failed to query keywords")
                .collect::<Result<Vec<String>>>()
                .expect("failed to collect keywords");

            e
        })
        .collect()
}

pub fn get_emails_in_mailbox(conn: &Connection, mailbox_id: &str) -> Vec<EmailRow> {
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

pub fn get_all_emails(conn: &Connection) -> Vec<EmailRow> {
    query_emails(
        conn,
        "SELECT id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment, is_read, is_flagged, size
         FROM email ORDER BY received_at DESC",
        &[],
    )
}

pub fn get_email(conn: &Connection, email_id: &str) -> Option<EmailRow> {
    let results = query_emails(
        conn,
        "SELECT id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment, is_read, is_flagged, size
         FROM email WHERE id = ?1",
        &[&email_id as &dyn rusqlite::ToSql],
    );
    results.into_iter().next()
}

pub fn get_thread(conn: &Connection, thread_id: &str) -> Vec<EmailRow> {
    query_emails(
        conn,
        "SELECT id, thread_id, subject, from_name, from_email, preview,
                received_at, has_attachment, is_read, is_flagged, size
         FROM email WHERE thread_id = ?1 ORDER BY received_at ASC",
        &[&thread_id as &dyn rusqlite::ToSql],
    )
}
