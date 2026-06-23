use super::helpers::automation_query_rule;
use super::*;

impl MailService {
    pub(crate) async fn apply_automation_rules(
        &self,
        account_id: &AccountId,
        messages: &[MessageRecord],
        _gateway: &dyn MailGateway,
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
                        .apply_automation_action(account_id, &message, action)
                        .await?;
                    events.extend(result.events);
                }
            }
        }
        Ok(events)
    }

    pub(super) async fn apply_automation_action(
        &self,
        account_id: &AccountId,
        message: &MessageSummary,
        action: &AutomationAction,
    ) -> Result<CommandAck, ServiceError> {
        match action {
            AutomationAction::ApplyTag { tag } => {
                if message.keywords.iter().any(|keyword| keyword == tag) {
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec![tag.clone()],
                        remove: Vec::new(),
                    },
                )
                .await
            }
            AutomationAction::RemoveTag { tag } => {
                if !message.keywords.iter().any(|keyword| keyword == tag) {
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec![tag.clone()],
                    },
                )
                .await
            }
            AutomationAction::MarkRead => {
                if message.is_read {
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec!["$seen".to_string()],
                        remove: Vec::new(),
                    },
                )
                .await
            }
            AutomationAction::MarkUnread => {
                if !message.is_read {
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec!["$seen".to_string()],
                    },
                )
                .await
            }
            AutomationAction::Flag => {
                if message.is_flagged {
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: vec!["$flagged".to_string()],
                        remove: Vec::new(),
                    },
                )
                .await
            }
            AutomationAction::Unflag => {
                if !message.is_flagged {
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.set_keywords(
                    account_id,
                    &message.id,
                    &SetKeywordsCommand {
                        add: Vec::new(),
                        remove: vec!["$flagged".to_string()],
                    },
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
                    return Ok(CommandAck { events: Vec::new() });
                }
                self.replace_mailboxes(
                    account_id,
                    &message.id,
                    &ReplaceMailboxesCommand {
                        mailbox_ids: vec![mailbox_id.clone()],
                    },
                )
                .await
            }
        }
    }
}
