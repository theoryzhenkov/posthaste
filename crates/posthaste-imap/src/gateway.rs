use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use imap_client::client::tokio::Client as ImapClient;
use posthaste_domain_model::{FetchedBody, GatewayError, Identity, ImapCapabilities, ImapMailboxSyncPlan, ImapMailboxSyncState, ImapMessageLocation, ImapMoveStrategy, ImapUid, ImapUidValidity, MutationOutcome, ProviderProfile, ReplyContext, SendMessageRequest, SetKeywordsCommand, StoreError, SyncBatch, SyncCursor, SyncProgress, SyncProgressStage, SyncTrigger, now_iso8601};
use posthaste_domain_model::{AccountId, BlobId, MailboxId, MessageId};
use posthaste_domain_service::{MailGateway, MailStore, PushTransport, SecretResolver, SyncProgressReporter, plan_imap_mailbox_sync, plan_imap_move};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};

use crate::discovery::connect_authenticated_client;
use crate::fetch::{
    fetch_mailbox_changed_since_snapshot_with_client, fetch_mailbox_header_snapshot_with_client,
    fetch_mailbox_headers_after_uid_with_client,
};
use crate::mailbox::{examine_selected_mailbox, status_imap_mailbox, ImapMailboxStatus};
use crate::{
    append_smtp_sent_copy, apply_imap_keyword_delta_by_location,
    copy_imap_message_to_mailbox_by_location, discover_imap_account,
    expunge_imap_message_by_location, fetch_imap_reply_context_by_location,
    fetch_message_body_by_location, fetch_raw_message_by_location,
    imap_attachment_bytes_from_raw_mime, imap_condstore_delta_sync_batch, imap_delta_sync_batch,
    imap_full_sync_batch, imap_mailbox_replacement_delta,
    imap_mailbox_state_from_changed_since_snapshot, imap_mailbox_state_from_header_snapshot,
    mark_imap_message_deleted_by_location, move_imap_message_to_mailbox_by_location,
    normalize_imap_capabilities, parse_imap_attachment_blob_id, smtp_sent_copy_strategy,
    submit_smtp_message, DiscoveredImapAccount, DiscoveredImapMailbox, ImapAdapterError,
    ImapChangedSinceSnapshot, ImapConnectionConfig, ImapMailboxHeaderSnapshot,
    ImapMailboxUidDeltaSnapshot, ImapMappedHeader, SmtpConnectionConfig, SmtpSentCopyStrategy,
};

/// Live IMAP/SMTP gateway after successful IMAP discovery.
///
/// The first implementation performs conservative full metadata snapshots.
/// Mutations use conservative IMAP commands where implemented and reject
/// unsupported command surfaces with typed gateway errors.
mod draft;
mod execution;
mod identity;
mod mail_gateway;
mod mutations;
mod planning;
mod progress;
mod send;
mod sync;
mod types;
mod utils;

pub use types::LiveImapSmtpGateway;

use draft::{delete_imap_draft, save_imap_draft};
use execution::{execute_mailbox_plans, MailboxPlanExecutionContext};
use identity::{
    imap_sync_plan_name, missing_location_identities, missing_location_identities_from_uids,
    planned_imap_sync_plan_name,
};
use mutations::{destroy_message_by_imap, replace_message_mailboxes};
use planning::{
    plan_mailboxes, planned_mailboxes_include_full_snapshot,
    planned_mailboxes_require_partial_delta_batch, simple_imap_move_mailboxes,
};
use progress::{report_sync_progress, ImapSyncProgressUpdate};
use send::send_message_via_smtp;
use sync::sync_imap_account;
use types::{PlannedImapMailbox, PlannedImapMailboxSync, SyncBatchAccumulator};
use utils::{imap_error_to_gateway, mailbox_status_proves_unchanged, store_error_to_gateway};

#[cfg(test)]
mod tests;
