//! SQLite-backed `MailStore` implementation: sync batch writes, lazy body
//! fetching, conversation projections, smart mailbox queries, and event log.
//!
//! @spec docs/L1-sync#sqlite-schema

mod automation;
mod cache;
mod commands;
mod db;
mod imap;
mod mutations;
mod outbox;
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

pub use crate::store::{DatabaseStore, RepairReport, REPAIR_MARKER_FILE};
pub(crate) use crate::store::StagedBodyFiles;
pub use rev_log::MAX_REV_LOG_HISTORY;

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use hex::encode as hex_encode;
use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, synthesize_plain_text_raw_mime, AccountId,
    AutomationBackfillJob, AutomationBackfillJobStatus, CacheCandidate, CacheFetchCandidate,
    CacheFetchUnit, CacheLayer, CacheObjectState, CachePriorityUpdate, CacheRescoreCandidate,
    CacheSearchSignals, CacheSignalUpdate, CachedSenderAddress, CommandResult, ConversationCursor,
    ConversationId, ConversationPage, ConversationSortField, ConversationSummary, ConversationView,
    DomainEvent, EventFilter, FetchedBody, ImapMailboxSyncState, ImapMessageLocation,
    ImapMessageLocationKey, ImapModSeq, ImapUid, ImapUidValidity, MailboxId, MailboxRole,
    MailboxSummary, MessageCursor, MessageDetail, MessageId, MessagePage, MessageSortField,
    MessageSummary, Operation, OperationEntity, OperationEntityKind, OperationId, OperationKind,
    OperationState, RawMessageRef, Recipient, ReplaceMailboxesCommand, RevCursor, RevLogSnapshot,
    RevLogStep, SetKeywordsCommand, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxValue, SortDirection, StoreError, SyncBatch, SyncCursor, SyncObject,
    SyncReconciliation, TagSummary, ThreadId, ThreadView, EVENT_TOPIC_MAILBOX_UPDATED,
    EVENT_TOPIC_MESSAGE_BODY_CACHED, EVENT_TOPIC_MESSAGE_UPDATED,
};
use posthaste_domain_service::{
    cache_signal_rescore_priority, AutomationBackfillStore, CacheStore, ConversationReadStore,
    EventStore, ImapMessageLocationStore, ImapMessageLocationWriteStore, ImapSyncStateStore,
    ImapSyncStateWriteStore, MailboxReadStore, MailboxRoleOverrideStore, MessageCommandStore,
    MessageDetailStore, MessageListStore, MessageMailboxStore, OperationOutboxStore, RevLogStore,
    SenderAddressCacheStore, SmartMailboxStore, SourceDataStore, SourceProjectionStore,
    SyncStateStore, SyncWriteStore, TagReadStore,
};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::db::{
    bool_to_i64, configure_connection, init_schema, io_to_store_error, json_to_store_error,
    now_iso8601, parse_sync_object, sql_to_store_error,
};
use crate::mutations::{
    apply_message_body_tx, apply_sync_batch_protected_tx, apply_sync_batch_tx, destroy_message_tx,
    list_events as list_events_for_filter, reconcile_sync_protected_tx, reconcile_sync_tx,
    replace_mailboxes_tx, set_keywords_tx, stage_sync_bodies,
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
