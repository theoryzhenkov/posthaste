use super::*;

impl AutomationBackfillStore for TestStore {
    fn ensure_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            return Ok(job.clone());
        }
        let job = AutomationBackfillJob {
            account_id: account_id.clone(),
            rule_fingerprint: rule_fingerprint.to_string(),
            status: AutomationBackfillJobStatus::Pending,
            attempts: 0,
            last_error: None,
            updated_at: crate::RFC3339_EPOCH.to_string(),
        };
        jobs.push(job.clone());
        Ok(job)
    }

    fn complete_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<(), StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            job.status = AutomationBackfillJobStatus::Completed;
            job.last_error = None;
        }
        Ok(())
    }

    fn record_automation_backfill_failure(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
        error: &str,
    ) -> Result<(), StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            job.status = AutomationBackfillJobStatus::Pending;
            job.attempts += 1;
            job.last_error = Some(error.to_string());
        }
        Ok(())
    }

    fn get_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<Option<AutomationBackfillJob>, StoreError> {
        Ok(self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned")
            .iter()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
            .cloned())
    }

    fn reset_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError> {
        let mut jobs = self
            .automation_backfill_jobs
            .lock()
            .expect("automation backfill jobs lock poisoned");
        if let Some(job) = jobs
            .iter_mut()
            .find(|job| &job.account_id == account_id && job.rule_fingerprint == rule_fingerprint)
        {
            job.status = AutomationBackfillJobStatus::Pending;
            job.attempts = 0;
            job.last_error = None;
            return Ok(job.clone());
        }
        let job = AutomationBackfillJob {
            account_id: account_id.clone(),
            rule_fingerprint: rule_fingerprint.to_string(),
            status: AutomationBackfillJobStatus::Pending,
            attempts: 0,
            last_error: None,
            updated_at: crate::RFC3339_EPOCH.to_string(),
        };
        jobs.push(job.clone());
        Ok(job)
    }
}
