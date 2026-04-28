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
mod projections;
mod query;
mod read;
mod sender_cache;
mod smart_mailboxes;
mod source;
mod store;
mod sync_state;

pub use crate::store::DatabaseStore;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use hex::encode as hex_encode;
use posthaste_domain::{
    cache_signal_rescore_priority, now_iso8601 as domain_now_iso8601,
    synthesize_plain_text_raw_mime, AccountId, AutomationBackfillJob, AutomationBackfillJobStatus,
    AutomationBackfillStore, CacheCandidate, CacheFetchCandidate, CacheFetchUnit, CacheLayer,
    CacheObjectState, CachePriorityUpdate, CacheRescoreCandidate, CacheSearchSignals,
    CacheSignalUpdate, CacheStore, CachedSenderAddress, CommandResult, ConversationCursor,
    ConversationId, ConversationPage, ConversationReadStore, ConversationSortField,
    ConversationSummary, ConversationView, DomainEvent, EventFilter, EventStore, FetchedBody,
    ImapMailboxSyncState, ImapMessageLocation, ImapMessageLocationStore,
    ImapMessageLocationWriteStore, ImapModSeq, ImapSyncStateStore, ImapSyncStateWriteStore,
    ImapUid, ImapUidValidity, MailboxId, MailboxReadStore, MailboxSummary, MessageCommandStore,
    MessageCursor, MessageDetail, MessageDetailStore, MessageId, MessageListStore,
    MessageMailboxStore, MessagePage, MessageSortField, MessageSummary, RawMessageRef, Recipient,
    ReplaceMailboxesCommand, SenderAddressCacheStore, SetKeywordsCommand, SmartMailboxCondition,
    SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxStore, SmartMailboxValue, SortDirection,
    SourceDataStore, SourceProjectionStore, StoreError, SyncBatch, SyncCursor, SyncObject,
    SyncStateStore, SyncWriteStore, TagReadStore, TagSummary, ThreadId, ThreadView,
    EVENT_TOPIC_MAILBOX_UPDATED, EVENT_TOPIC_MESSAGE_ARRIVED, EVENT_TOPIC_MESSAGE_BODY_CACHED,
    EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED, EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
    EVENT_TOPIC_MESSAGE_UPDATED,
};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use tracing::{debug, info};

use crate::db::{
    bool_to_i64, configure_connection, init_schema, io_to_store_error, json_to_store_error,
    now_iso8601, parse_sync_object, sql_to_store_error,
};
use crate::mutations::{
    apply_message_body_tx, apply_sync_batch_tx, destroy_message_tx,
    list_events as list_events_for_filter, replace_mailboxes_tx, set_keywords_tx,
    stage_sync_bodies,
};
use crate::projections::{cleanup_orphan_conversations_tx, insert_event_tx, synthesize_raw_mime};
use crate::query::{
    fetch_mailbox_ids, fetch_message_attachments, hydrate_message_summaries,
    load_message_summary_rows, row_to_message_summary_row,
};
use crate::smart_mailboxes::{
    count_smart_mailbox_messages, query_conversations, query_conversations_by_rule,
    query_message_page, query_message_page_by_rule, query_messages_by_rule,
};

#[cfg(test)]
mod tests;
