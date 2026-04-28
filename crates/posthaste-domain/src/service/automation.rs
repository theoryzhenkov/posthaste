use crate::{
    AccountId, AppSettings, AutomationAction, AutomationBackfillBatchOutcome,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger,
    CommandResult, DomainEvent, MailGateway, MessageId, MessageRecord, MessageSortField,
    MessageSummary, ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, SortDirection,
    StoreError,
};

use super::MailService;

fn condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: false,
        value,
    })
}

fn negated_condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: true,
        value,
    })
}

fn automation_query_rule(
    account_id: &AccountId,
    rule: &AutomationRule,
    action: &AutomationAction,
    message_ids: &[MessageId],
) -> SmartMailboxRule {
    let mut nodes = vec![
        condition_node(
            SmartMailboxField::SourceId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(account_id.to_string()),
        ),
        SmartMailboxRuleNode::Group(rule.condition.root.clone()),
    ];

    if !message_ids.is_empty() {
        nodes.push(condition_node(
            SmartMailboxField::MessageId,
            SmartMailboxOperator::In,
            SmartMailboxValue::Strings(message_ids.iter().map(ToString::to_string).collect()),
        ));
    }

    if let Some(precondition) = automation_action_precondition(action) {
        nodes.push(precondition);
    }

    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn automation_action_precondition(action: &AutomationAction) -> Option<SmartMailboxRuleNode> {
    match action {
        AutomationAction::ApplyTag { tag } => Some(negated_condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        AutomationAction::RemoveTag { tag } => Some(condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        AutomationAction::MarkRead => Some(condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
        )),
        AutomationAction::MarkUnread => Some(condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
        )),
        AutomationAction::Flag => Some(condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
        )),
        AutomationAction::Unflag => Some(condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
        )),
        AutomationAction::MoveToMailbox { mailbox_id } => Some(negated_condition_node(
            SmartMailboxField::MailboxId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(mailbox_id.to_string()),
        )),
    }
}

fn automation_backfill_fingerprint(settings: &AppSettings) -> Result<Option<String>, ServiceError> {
    let rules = settings
        .automation_rules
        .iter()
        .filter(|rule| rule.enabled && rule.backfill)
        .cloned()
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&rules).map(Some).map_err(|err| {
        StoreError::Failure(format!("failed to fingerprint automation rules: {err}")).into()
    })
}

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

    pub(super) async fn apply_automation_rules(
        &self,
        account_id: &AccountId,
        messages: &[MessageRecord],
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        if self.config.get_source(account_id)?.is_none() {
            return Ok(Vec::new());
        }
        let settings = self.config.get_app_settings()?;
        if settings.automation_rules.is_empty() || messages.is_empty() {
            return Ok(Vec::new());
        }

        let mut events = Vec::new();
        let message_ids = messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>();
        for rule in settings.automation_rules.iter().filter(|rule| {
            rule.enabled
                && rule
                    .triggers
                    .iter()
                    .any(|trigger| trigger == &AutomationTrigger::MessageArrived)
        }) {
            for action in &rule.actions {
                let query_rule = automation_query_rule(account_id, rule, action, &message_ids);
                let page = self.smart_mailboxes.query_message_page_by_rule(
                    &query_rule,
                    messages.len(),
                    None,
                    MessageSortField::Date,
                    SortDirection::Asc,
                )?;
                for message in page.items {
                    let result = self
                        .apply_automation_action(account_id, &message, action, gateway)
                        .await?;
                    events.extend(result.events);
                }
            }
        }
        Ok(events)
    }

    async fn apply_automation_action(
        &self,
        account_id: &AccountId,
        message: &MessageSummary,
        action: &AutomationAction,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        match action {
            AutomationAction::ApplyTag { tag } => {
                if message.keywords.iter().any(|keyword| keyword == tag) {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec![tag.clone()],
                        remove: Vec::new(),
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::RemoveTag { tag } => {
                if !message.keywords.iter().any(|keyword| keyword == tag) {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec![tag.clone()],
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::MarkRead => {
                if message.is_read {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec!["$seen".to_string()],
                        remove: Vec::new(),
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::MarkUnread => {
                if !message.is_read {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec!["$seen".to_string()],
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::Flag => {
                if message.is_flagged {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec!["$flagged".to_string()],
                        remove: Vec::new(),
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::Unflag => {
                if !message.is_flagged {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec!["$flagged".to_string()],
                    },
                    gateway,
                )
                .await
            }
            AutomationAction::MoveToMailbox { mailbox_id } => {
                if message.mailbox_ids.len() == 1
                    && message
                        .mailbox_ids
                        .iter()
                        .any(|candidate| candidate == mailbox_id)
                {
                    return Ok(CommandResult {
                        detail: None,
                        events: Vec::new(),
                    });
                }
                self.replace_mailboxes(
                    account_id,
                    &message.id,
                    &ReplaceMailboxesCommand {
                        mailbox_ids: vec![mailbox_id.clone()],
                    },
                    gateway,
                )
                .await
            }
        }
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

        let job = self
            .automation_backfills
            .ensure_automation_backfill_job(account_id, &rule_fingerprint)?;
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
                    self.automation_backfills
                        .complete_automation_backfill_job(account_id, &rule_fingerprint)?;
                }
                Ok(AutomationBackfillBatchOutcome {
                    ran: true,
                    events,
                    has_more,
                })
            }
            Err(error) => {
                self.automation_backfills
                    .record_automation_backfill_failure(
                        account_id,
                        &rule_fingerprint,
                        &error.to_string(),
                    )?;
                Err(error)
            }
        }
    }

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

    async fn backfill_automation_rules_batch_with_settings(
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
