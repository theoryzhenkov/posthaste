use crate::{
    AccountId, AppSettings, AutomationAction, AutomationBackfillBatchOutcome,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger,
    CommandAck, DomainEvent, MailGateway, MessageId, MessageRecord, MessageSortField,
    MessageSummary, ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, SortDirection,
    StoreError,
};

use super::MailService;

mod apply;
mod backfill;
mod helpers;
mod jobs;
