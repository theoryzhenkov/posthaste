use super::helpers::automation_backfill_fingerprint;
use super::*;

impl MailService {
    /// Ensure enabled accounts have a durable job for the current backfill rules.
    ///
    /// Completed jobs are preserved, so calling this on startup or after a
    /// settings PATCH is cheap unless the rule fingerprint changed.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub fn ensure_automation_backfills_for_current_rules(
        &self,
    ) -> Result<Vec<AutomationBackfillJob>, ServiceError> {
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(Vec::new());
        };
        self.config
            .list_sources()?
            .into_iter()
            .filter(|source| source.enabled)
            .map(|source| {
                self.automation_backfills
                    .ensure_automation_backfill_job(&source.id, &rule_fingerprint)
                    .map_err(Into::into)
            })
            .collect()
    }

    /// Force the current backfill rules to re-run for every enabled account.
    ///
    /// Unlike `ensure_*`, this resets completed jobs back to pending so an
    /// on-demand "backfill now" re-applies the rules even when nothing in the
    /// fingerprint changed. Returns an empty list when no rule is
    /// backfill-eligible (enabled + backfill).
    ///
    /// @spec docs/L1-sync#automation-actions
    pub fn reset_automation_backfills_for_current_rules(
        &self,
    ) -> Result<Vec<AutomationBackfillJob>, ServiceError> {
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(Vec::new());
        };
        self.config
            .list_sources()?
            .into_iter()
            .filter(|source| source.enabled)
            .map(|source| {
                self.automation_backfills
                    .reset_automation_backfill_job(&source.id, &rule_fingerprint)
                    .map_err(Into::into)
            })
            .collect()
    }

    /// Return the current-rules backfill job for an account, if applicable.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub fn automation_backfill_job_for_current_rules(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<AutomationBackfillJob>, ServiceError> {
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(None);
        };
        self.automation_backfills
            .get_automation_backfill_job(account_id, &rule_fingerprint)
            .map_err(Into::into)
    }

    /// Process one durable low-priority automation backfill batch for an account.
    ///
    /// The current rules are fingerprinted before work starts. A completed job
    /// suppresses repeated scans for the same rules, while changed rules create
    /// a new pending job.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub async fn process_automation_backfill_job_batch(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        batch_size: usize,
    ) -> Result<AutomationBackfillBatchOutcome, ServiceError> {
        if batch_size == 0 {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        }
        let Some(source) = self.config.get_source(account_id)? else {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        };
        if !source.enabled {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        }
        let settings = self.config.get_app_settings()?;
        let Some(rule_fingerprint) = automation_backfill_fingerprint(&settings)? else {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        };

        // Job bookkeeping is synchronous store work; run it on the blocking
        // pool so the caller's async loop is never stalled by SQLite.
        let job = {
            let automation_backfills = self.automation_backfills.clone();
            let account_id = account_id.clone();
            let rule_fingerprint = rule_fingerprint.clone();
            offload(move || {
                automation_backfills.ensure_automation_backfill_job(&account_id, &rule_fingerprint)
            })
            .await?
        };
        if job.status != AutomationBackfillJobStatus::Pending {
            return Ok(AutomationBackfillBatchOutcome {
                ran: false,
                events: Vec::new(),
                has_more: false,
            });
        }

        match self
            .backfill_automation_rules_batch_with_settings(
                account_id, gateway, batch_size, &settings,
            )
            .await
        {
            Ok((events, has_more)) => {
                if !has_more {
                    let automation_backfills = self.automation_backfills.clone();
                    let account_id = account_id.clone();
                    let rule_fingerprint = rule_fingerprint.clone();
                    offload(move || {
                        automation_backfills
                            .complete_automation_backfill_job(&account_id, &rule_fingerprint)
                    })
                    .await?;
                }
                Ok(AutomationBackfillBatchOutcome {
                    ran: true,
                    events,
                    has_more,
                })
            }
            Err(error) => {
                let automation_backfills = self.automation_backfills.clone();
                let account_id = account_id.clone();
                let rule_fingerprint = rule_fingerprint.clone();
                let message = error.to_string();
                offload(move || {
                    automation_backfills.record_automation_backfill_failure(
                        &account_id,
                        &rule_fingerprint,
                        &message,
                    )
                })
                .await?;
                Err(error)
            }
        }
    }
}
