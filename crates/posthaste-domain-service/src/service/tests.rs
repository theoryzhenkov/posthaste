use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::*;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AppSettings, AutomationAction,
    AutomationBackfillJob, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger,
    CacheCandidate, CacheFetchCandidate, CacheFetchLease, CacheFetchUnit, CacheLayer,
    CacheObjectState, CachePolicy, CachePriorityUpdate, CacheRescoreCandidate, CacheSignalUpdate,
    CachedSenderAddress, CommandResult, ConfigError, ConversationCursor, ConversationId,
    ConversationPage, ConversationSortField, ConversationView, DomainEvent, EventFilter,
    FetchedBody, GatewayError, Identity, ImapMailboxSyncState, ImapMessageLocation,
    MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryGroupOperator, MailQueryOperator,
    MailQueryRule, MailQueryRuleNode, MailQueryValue, MailboxId, MailboxSummary, MessageCursor,
    MessageDetail, MessageId, MessagePage, MessageRecord, MessageSortField, MessageSummary,
    MutationOutcome, Operation, OperationEntity, OperationEntityKind, OperationId, OperationKind,
    OperationOutcome, OperationSettlement, OperationState, Recipient, ReplaceMailboxesCommand,
    RevLogSnapshot, SendMessageRequest, SetKeywordsCommand, SmartMailbox, SmartMailboxId,
    SmartMailboxKind, SortDirection, StoreError, SyncBatch, SyncCursor, SyncMode, SyncObject,
    SyncTrigger, TagSummary, ThreadId, ThreadView, EVENT_TOPIC_MAILBOX_UPDATED,
    EVENT_TOPIC_MESSAGE_UPDATED, EVENT_TOPIC_OPERATION_SETTLED,
};

mod config;
mod fixtures;
mod mutation_gateway;
mod store;
mod store_automation_impls;
mod store_command_event_impls;
mod store_read_impls;
mod store_sync_cache_impls;

use config::*;
use fixtures::*;
use mutation_gateway::*;
use store::*;

mod automation;
mod body_cache_budget;
mod body_cache_worker;
mod cache_rescore;
mod identity_fallback;
mod mailbox_role;
mod message_mutation_cursors;
mod message_mutation_retries;
mod message_queries;
mod offload;
mod outbox;
mod replay;
mod send_alias;
mod smart_mailboxes;
mod snooze;
mod source_cleanup;
mod sync_cache_candidates;
