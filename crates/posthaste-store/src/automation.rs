use super::*;

fn parse_automation_backfill_status(
    value: String,
) -> Result<AutomationBackfillJobStatus, StoreError> {
    match value.as_str() {
        "pending" => Ok(AutomationBackfillJobStatus::Pending),
        "completed" => Ok(AutomationBackfillJobStatus::Completed),
        other => Err(StoreError::Failure(format!(
            "unknown automation backfill job status: {other}"
        ))),
    }
}

impl AutomationBackfillStore for DatabaseStore {
    fn ensure_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError> {
        self.write_transaction(|tx| {
            let timestamp = now_iso8601()?;
            tx.execute(
                "INSERT OR IGNORE INTO automation_backfill_job (
                    account_id, rule_fingerprint, status, attempts, last_error, queued_at, updated_at
                 )
                 VALUES (?1, ?2, 'pending', 0, NULL, ?3, ?3)",
                params![account_id.as_str(), rule_fingerprint, timestamp],
            )
            .map_err(sql_to_store_error)?;
            fetch_automation_backfill_job(tx, account_id, rule_fingerprint)?
                .ok_or_else(|| StoreError::Failure("automation backfill job was not created".to_string()))
        })
    }

    fn complete_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE automation_backfill_job
                 SET status = 'completed', last_error = NULL, updated_at = ?3
                 WHERE account_id = ?1 AND rule_fingerprint = ?2",
                params![account_id.as_str(), rule_fingerprint, now_iso8601()?],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn record_automation_backfill_failure(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
        error: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "UPDATE automation_backfill_job
                 SET status = 'pending',
                     attempts = attempts + 1,
                     last_error = ?3,
                     updated_at = ?4
                 WHERE account_id = ?1 AND rule_fingerprint = ?2",
                params![account_id.as_str(), rule_fingerprint, error, now_iso8601()?],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn get_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<Option<AutomationBackfillJob>, StoreError> {
        let connection = self.read_connection()?;
        fetch_automation_backfill_job(&connection, account_id, rule_fingerprint)
    }
}

fn fetch_automation_backfill_job(
    connection: &Connection,
    account_id: &AccountId,
    rule_fingerprint: &str,
) -> Result<Option<AutomationBackfillJob>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT account_id, rule_fingerprint, status, attempts, last_error, updated_at
             FROM automation_backfill_job
             WHERE account_id = ?1 AND rule_fingerprint = ?2",
        )
        .map_err(sql_to_store_error)?;
    statement
        .query_row(params![account_id.as_str(), rule_fingerprint], |row| {
            let status: String = row.get(2)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                status,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .optional()
        .map_err(sql_to_store_error)?
        .map(
            |(account_id, rule_fingerprint, status, attempts, last_error, updated_at)| {
                Ok(AutomationBackfillJob {
                    account_id: AccountId::from(account_id),
                    rule_fingerprint,
                    status: parse_automation_backfill_status(status)?,
                    attempts,
                    last_error,
                    updated_at,
                })
            },
        )
        .transpose()
}
