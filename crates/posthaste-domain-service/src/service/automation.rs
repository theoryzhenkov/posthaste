use crate::MailGateway;
use posthaste_domain_model::{
    AccountId, AppSettings, AutomationAction, AutomationBackfillBatchOutcome,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger,
    CommandAck, DomainEvent, MailQueryCondition, MailQueryField, MailQueryGroup,
    MailQueryGroupOperator, MailQueryOperator, MailQueryRule, MailQueryRuleNode, MailQueryValue,
    MessageId, MessageRecord, MessageSortField, MessageSummary, ReplaceMailboxesCommand,
    ServiceError, SetKeywordsCommand, SortDirection, StoreError,
};

use super::{offload, MailService};

mod apply;
mod backfill;
mod helpers;
mod jobs;
