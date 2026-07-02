use crate::MailGateway;
use posthaste_domain_model::{
    AccountId, AppSettings, AutomationAction, AutomationBackfillBatchOutcome,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger,
    CommandAck, DomainEvent, MessageId, MessageRecord, MessageSortField, MessageSummary,
    ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand, SmartMailboxCondition,
    SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, SortDirection, StoreError,
};

use super::MailService;

mod apply;
mod backfill;
mod helpers;
mod jobs;
