use super::helpers::automation_query_rule;
use super::*;

impl MailService {
    /// Apply one bounded batch of global automation rules to existing local mail.
    ///
    /// This is intended for low-priority background backfill. It queries the
    /// local projection first, then applies actions through JMAP so the server
    /// remains authoritative.
    ///
    /// @spec docs/L1-sync#automation-actions
    pub async fn backfill_automation_rules_batch(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        batch_size: usize,
    ) -> Result<(Vec<DomainEvent>, bool), ServiceError> {
        if self.config.get_source(account_id)?.is_none() {
            return Ok((Vec::new(), false));
        }
        let settings = self.config.get_app_settings()?;
        self.backfill_automation_rules_batch_with_settings(
            account_id, gateway, batch_size, &settings,
        )
        .await
    }

    pub(crate) async fn backfill_automation_rules_batch_with_settings(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        batch_size: usize,
        settings: &AppSettings,
    ) -> Result<(Vec<DomainEvent>, bool), ServiceError> {
        if settings.automation_rules.is_empty() || batch_size == 0 {
            return Ok((Vec::new(), false));
        }

        let mut events = Vec::new();
        let mut has_more = false;
        let mut remaining = batch_size;

        for rule in settings
            .automation_rules
            .iter()
            .filter(|rule| rule.enabled && rule.backfill)
        {
            if remaining == 0 {
                has_more = true;
                break;
            }
            for action in &rule.actions {
                if remaining == 0 {
                    has_more = true;
                    break;
                }
                let query_rule = automation_query_rule(account_id, rule, action, &[]);
                let page = self.smart_mailboxes.query_message_page_by_rule(
                    &query_rule,
                    remaining,
                    None,
                    MessageSortField::Date,
                    SortDirection::Asc,
                )?;
                if page.items.len() == remaining {
                    has_more = true;
                }

                for message in page.items {
                    let result = self
                        .apply_automation_action(account_id, &message, action, gateway)
                        .await?;
                    if !result.events.is_empty() {
                        remaining -= 1;
                    }
                    events.extend(result.events);
                    if remaining == 0 {
                        has_more = true;
                        break;
                    }
                }
            }
        }

        Ok((events, has_more))
    }
}
