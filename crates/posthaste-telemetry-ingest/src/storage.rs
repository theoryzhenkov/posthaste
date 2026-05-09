use std::{path::Path, sync::Mutex};

use rusqlite::{params, Connection};
use time::OffsetDateTime;

use crate::schema::TelemetryBatch;

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestOutcome {
    pub accepted_events: usize,
    pub duplicate_events: usize,
}

pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        apply_schema(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn ready(&self) -> bool {
        let Ok(connection) = self.connection.lock() else {
            return false;
        };
        connection.query_row("SELECT 1", [], |_| Ok(())).is_ok()
    }

    pub fn ingest(&self, batch: &TelemetryBatch) -> Result<IngestOutcome, StoreError> {
        let mut connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let transaction = connection.transaction()?;
        let received_at = OffsetDateTime::now_utc().unix_timestamp();
        transaction.execute(
            "INSERT INTO raw_batches (
                received_at, schema_version, app_version, app_channel, os_family, arch,
                telemetry_mode, client_day, subject_id, event_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                received_at,
                batch.schema_version,
                batch.app_version,
                batch.app_channel.as_str(),
                batch.os_family.as_str(),
                batch.arch.as_str(),
                batch.telemetry_mode.as_str(),
                batch.client_day,
                batch.subject_id,
                batch.events.len() as i64,
            ],
        )?;
        let batch_id = transaction.last_insert_rowid();

        let mut outcome = IngestOutcome::default();
        for event in &batch.events {
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO event_dedupe (event_id, first_seen_at) VALUES (?1, ?2)",
                params![event.event_id, received_at],
            )?;
            if inserted == 0 {
                outcome.duplicate_events += 1;
                continue;
            }

            transaction.execute(
                "INSERT INTO raw_events (
                    batch_id, event_id, received_at, event_name, event_version,
                    app_version, app_channel, os_family, arch, telemetry_mode,
                    client_day, subject_id, fields_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    batch_id,
                    event.event_id,
                    received_at,
                    event.name,
                    event.version,
                    batch.app_version,
                    batch.app_channel.as_str(),
                    batch.os_family.as_str(),
                    batch.arch.as_str(),
                    batch.telemetry_mode.as_str(),
                    batch.client_day,
                    batch.subject_id,
                    serde_json::to_string(&event.fields)?,
                ],
            )?;
            outcome.accepted_events += 1;
        }

        transaction.commit()?;
        Ok(outcome)
    }

    pub fn apply_retention(
        &self,
        raw_retention_days: i64,
        dedupe_retention_days: i64,
    ) -> Result<(), StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let raw_cutoff = now - raw_retention_days.max(1) * 86_400;
        let dedupe_cutoff = now - dedupe_retention_days.max(1) * 86_400;
        connection.execute(
            "DELETE FROM raw_events WHERE received_at < ?1",
            params![raw_cutoff],
        )?;
        connection.execute(
            "DELETE FROM raw_batches WHERE received_at < ?1",
            params![raw_cutoff],
        )?;
        connection.execute(
            "DELETE FROM event_dedupe WHERE first_seen_at < ?1",
            params![dedupe_cutoff],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn event_count(&self) -> Result<i64, StoreError> {
        let connection = self.connection.lock().map_err(|_| StoreError::Poisoned)?;
        connection
            .query_row("SELECT COUNT(*) FROM raw_events", [], |row| row.get(0))
            .map_err(StoreError::from)
    }
}

fn apply_schema(connection: &Connection) -> Result<(), StoreError> {
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS raw_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            received_at INTEGER NOT NULL,
            schema_version INTEGER NOT NULL,
            app_version TEXT NOT NULL,
            app_channel TEXT NOT NULL,
            os_family TEXT NOT NULL,
            arch TEXT NOT NULL,
            telemetry_mode TEXT NOT NULL,
            client_day TEXT NOT NULL,
            subject_id TEXT,
            event_count INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS raw_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_id INTEGER NOT NULL REFERENCES raw_batches(id) ON DELETE CASCADE,
            event_id TEXT NOT NULL,
            received_at INTEGER NOT NULL,
            event_name TEXT NOT NULL,
            event_version INTEGER NOT NULL,
            app_version TEXT NOT NULL,
            app_channel TEXT NOT NULL,
            os_family TEXT NOT NULL,
            arch TEXT NOT NULL,
            telemetry_mode TEXT NOT NULL,
            client_day TEXT NOT NULL,
            subject_id TEXT,
            fields_json TEXT NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS raw_events_event_id_idx ON raw_events(event_id);
        CREATE INDEX IF NOT EXISTS raw_events_name_day_idx ON raw_events(event_name, client_day);
        CREATE INDEX IF NOT EXISTS raw_events_received_at_idx ON raw_events(received_at);

        CREATE TABLE IF NOT EXISTS event_dedupe (
            event_id TEXT PRIMARY KEY,
            first_seen_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS event_dedupe_first_seen_at_idx ON event_dedupe(first_seen_at);
        "#,
    )?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem error")]
    Io(#[from] std::io::Error),
    #[error("serialization error")]
    Serde(#[from] serde_json::Error),
    #[error("database lock poisoned")]
    Poisoned,
}
