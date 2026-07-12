//! SQLite-backed `MailStore` implementation: sync batch writes, lazy body
//! fetching, conversation projections, smart mailbox queries, and event log.
//!
//! @spec docs/L1-sync#sqlite-schema

mod apply_ledger;
mod automation;
mod cache;
mod commands;
mod db;
mod imap;
mod mutations;
mod outbox;
mod overlay;
mod projections;
mod query;
mod read;
mod rev_log;
mod sender_cache;
mod smart_mailboxes;
mod snooze;
mod source;
mod sql_cache;
mod store;
mod sync_state;
#[cfg(test)]
mod test_support;

pub use crate::apply_ledger::{
    ApplyLedgerReserve, ApplyLedgerRow, ApplyLedgerState, APPLY_LEDGER_RETENTION_SECS,
};
pub(crate) use crate::store::StagedBodyFiles;
pub use crate::store::{DatabaseStore, RepairReport, REPAIR_MARKER_FILE};
pub use rev_log::MAX_REV_LOG_HISTORY;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use hex::encode as hex_encode;
use posthaste_domain_model::{
    field_spec, now_iso8601 as domain_now_iso8601, synthesize_plain_text_raw_mime, AccountId,
    AutomationBackfillJob, AutomationBackfillJobStatus, CacheCandidate, CacheFetchCandidate,
    CacheFetchUnit, CacheLayer, CacheObjectState, CachePriorityUpdate, CacheRescoreCandidate,
    CacheSearchSignals, CacheSignalUpdate, CachedSenderAddress, CommandResult, ConversationCursor,
    ConversationId, ConversationPage, ConversationSortField, ConversationSummary, ConversationView,
    DateUnit, DateValue, DomainEvent, EventFilter, EventLogBounds, FetchedBody,
    ImapMailboxSyncState, ImapMessageLocation, ImapMessageLocationKey, ImapModSeq, ImapUid,
    ImapUidValidity, ListUnsubscribe, MailQueryCondition, MailQueryField, MailQueryGroup,
    MailQueryGroupOperator, MailQueryOperator, MailQueryRule, MailQueryRuleNode, MailQueryValue,
    MailboxId, MailboxRole, MailboxSummary, MessageCursor, MessageDetail, MessageId, MessagePage,
    MessageSortField, MessageSummary, Operation, OperationEntity, OperationEntityKind, OperationId,
    OperationKind, OperationState, RawMessageRef, Recipient, RevCursor, RevLogSnapshot, RevLogStep,
    SortDirection, StoreError, SyncBatch, SyncCursor, SyncObject, SyncReconciliation, TagSummary,
    ThreadId, ThreadView, EVENT_TOPIC_MAILBOX_UPDATED, EVENT_TOPIC_MESSAGE_BODY_CACHED,
    EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_domain_service::{
    cache_signal_rescore_priority, AutomationBackfillStore, BaseWrite, CacheStore,
    ConversationReadStore, DraftRegistry, EventStore, ImapMessageLocationStore,
    ImapMessageLocationWriteStore, ImapSyncStateStore, ImapSyncStateWriteStore, MailboxReadStore,
    MailboxRoleOverrideStore, MessageDetailStore, MessageListStore, MessageMailboxStore,
    MessageOverlayStore, OperationOutboxStore, RevLogStore, SenderAddressCacheStore,
    SmartMailboxStore, SourceDataStore, SourceProjectionStore, SyncStateStore, SyncWriteStore,
    TagReadStore,
};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::db::{
    bool_to_i64, configure_connection, io_to_store_error, json_to_store_error, now_iso8601,
    parse_sync_object, prepare_schema, sql_to_store_error,
};
use crate::mutations::{
    apply_message_body_tx, apply_sync_batch_tx, event_log_bounds as event_log_bounds_query,
    list_events as list_events_for_filter, reconcile_sync_tx, stage_sync_bodies,
};
use crate::projections::{cleanup_orphan_conversations_tx, insert_event_tx, synthesize_raw_mime};
use crate::query::{
    fetch_mailbox_ids, fetch_message_attachments, hydrate_message_summaries,
    load_message_summary_rows, row_to_message_summary_row,
};
use crate::smart_mailboxes::{
    count_smart_mailbox_messages, query_conversations, query_conversations_by_rule,
    query_message_page, query_message_page_by_rule, query_messages_by_rule,
    query_messages_by_rule_sorted,
};

#[cfg(test)]
mod tests;
