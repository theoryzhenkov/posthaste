use std::fmt::{Display, Formatter};
use std::pin::Pin;

use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::{
    cache::{CacheFetchUnit, CachePolicy},
    imap::{ImapMailboxSyncState, ImapMessageLocation, ImapMessageLocationKey},
    ConfigError, ProviderKind,
};

/// Generates a newtype wrapper around `String` for type-safe identifiers.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id!(
    /// Opaque server-assigned identifier for a mail account.
    ///
    /// @spec docs/L0-accounts#the-invariant
    AccountId
);

string_id!(
    /// Opaque server-assigned identifier for a mailbox (folder or label).
    ///
    /// @spec docs/L1-jmap#core-types
    MailboxId
);

string_id!(
    /// Opaque server-assigned identifier for a single email message.
    ///
    /// @spec docs/L1-jmap#core-types
    MessageId
);

string_id!(
    /// Opaque server-assigned identifier for a JMAP thread.
    ///
    /// @spec docs/L1-jmap#core-types
    ThreadId
);

string_id!(
    /// Opaque server-assigned identifier for a binary blob (attachment or body content).
    ///
    /// @spec docs/L1-jmap#methods-used
    BlobId
);

string_id!(
    /// Locally-derived identifier for a conversation (cross-source thread grouping).
    ///
    /// @spec docs/L1-sync#conversation-pagination
    ConversationId
);

string_id!(
    /// Identifier for a smart mailbox (saved query with display metadata).
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    SmartMailboxId
);

/// Default timestamp for missing `created_at`/`updated_at` fields in config.
///
/// @spec docs/L1-accounts#toml-schema
pub const RFC3339_EPOCH: &str = "1970-01-01T00:00:00Z";

/// MIME header carrying a draft's stable client identity, written when saving a
/// draft and read back on sync into [`MessageRecord::draft_id`]. It survives the
/// provider id rotation a JMAP draft update causes, so a resumed draft is keyed
/// by a stable value rather than the rotating provider id.
///
/// @spec docs/L1-outbox#temp-id-reconciliation
pub const DRAFT_ID_HEADER: &str = "X-Posthaste-Draft-Id";

/// Event topic emitted after a successful sync cycle completes.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_SYNC_COMPLETED: &str = "sync.completed";

/// Event topic emitted when a sync cycle fails.
///
/// @spec docs/L1-sync#error-handling
pub const EVENT_TOPIC_SYNC_FAILED: &str = "sync.failed";

/// Event topic emitted when application settings change.
///
/// @spec docs/L1-api#settings
pub const EVENT_TOPIC_SETTINGS_UPDATED: &str = "settings.updated";

/// Event topic emitted after an external config reload.
///
/// @spec docs/L1-api#sync-and-events
pub const EVENT_TOPIC_CONFIG_RELOADED: &str = "config.reloaded";

/// Event topic emitted when a smart mailbox is created.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_CREATED: &str = "smart_mailbox.created";

/// Event topic emitted when a smart mailbox is updated.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_UPDATED: &str = "smart_mailbox.updated";

/// Event topic emitted when a smart mailbox is deleted.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_DELETED: &str = "smart_mailbox.deleted";

/// Event topic emitted when default smart mailboxes are reset.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub const EVENT_TOPIC_SMART_MAILBOX_RESET: &str = "smart_mailbox.reset";

/// Event topic emitted when message metadata changes (keywords, mailboxes).
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_UPDATED: &str = "message.updated";

/// Event topic emitted when a message body is cached locally.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_BODY_CACHED: &str = "message.body_cached";

/// Event topic emitted when a Phase 2 reversible-op step is appended to the
/// per-account `rev_log` (a forward action confirmed). Drives the `RevLog`
/// synced view to re-serve the log + cursor.
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
pub const EVENT_TOPIC_REV_LOG_APPENDED: &str = "rev_log.appended";

/// Event topic emitted when a mailbox is created, updated, or deleted.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MAILBOX_UPDATED: &str = "mailbox.updated";

/// Event topic emitted when account configuration changes.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_UPDATED: &str = "account.updated";

/// Event topic emitted when a new account is created.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_CREATED: &str = "account.created";

/// Event topic emitted when an account is deleted.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_DELETED: &str = "account.deleted";

/// Event topic emitted when account runtime status transitions.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_STATUS_CHANGED: &str = "account.status_changed";

/// Event topic emitted when a push transport connects successfully.
///
/// @spec docs/L2-transport#push-transport
pub const EVENT_TOPIC_PUSH_CONNECTED: &str = "push.connected";

/// Event topic emitted when a push transport disconnects or fails.
///
/// @spec docs/L2-transport#push-transport
pub const EVENT_TOPIC_PUSH_DISCONNECTED: &str = "push.disconnected";

/// Event topic emitted when an outbox operation reaches a terminal outcome
/// (applied or failed) so a downstream tier can settle its optimistic state.
/// Payload is an [`OperationSettlement`].
///
/// @spec docs/L1-outbox#settlement
pub const EVENT_TOPIC_OPERATION_SETTLED: &str = "operation.settled";

/// Event topic emitted when a **send** is parked because its delivery outcome
/// is unknown (timeout/transport-loss after the submission may have committed,
/// or an interrupted flush). Payload is an [`OperationDispatchUncertain`]. It is
/// a needs-attention fact, not a settlement: the message may or may not have
/// been delivered, and the user must confirm (retry) or discard.
///
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
pub const EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN: &str = "operation.dispatch_uncertain";

/// Event topic emitted every time an automation **rule** fires — its WHEN-clause
/// matched a triggering fact and its action was executed. Payload is a
/// [`RuleFired`]. A rule-action invocation is itself a fact (RFC-L2-scripting
/// §8): scriptable and auditable through the same tap as every other event.
///
/// @spec docs/eph/RFC-L2-scripting#8-rules-run-at-the-authority-server
pub const EVENT_TOPIC_RULE_FIRED: &str = "rule.fired";

/// Event topic emitted when a rule's webhook/exec delivery is abandoned after
/// its bounded retry schedule is exhausted — the dead-letter fact (RFC-L2-scripting
/// ruling 5). Payload is a [`RuleDeliveryFailed`]. Delivery state is itself
/// observable on the tap; a consumer can react to a failed delivery.
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings (ruling 5)
pub const EVENT_TOPIC_RULE_DELIVERY_FAILED: &str = "rule.delivery.failed";

/// Every event topic the server emits, in declaration order.
///
/// Single source of truth for the documented topic set: the committed
/// `asyncapi.json` event contract is drift-checked against this slice.
///
/// @spec docs/L1-api#sse-event-stream
pub const ALL_EVENT_TOPICS: &[&str] = &[
    EVENT_TOPIC_SYNC_COMPLETED,
    EVENT_TOPIC_SYNC_FAILED,
    EVENT_TOPIC_SETTINGS_UPDATED,
    EVENT_TOPIC_CONFIG_RELOADED,
    EVENT_TOPIC_SMART_MAILBOX_CREATED,
    EVENT_TOPIC_SMART_MAILBOX_UPDATED,
    EVENT_TOPIC_SMART_MAILBOX_DELETED,
    EVENT_TOPIC_SMART_MAILBOX_RESET,
    EVENT_TOPIC_MESSAGE_UPDATED,
    EVENT_TOPIC_MESSAGE_BODY_CACHED,
    EVENT_TOPIC_MAILBOX_UPDATED,
    EVENT_TOPIC_ACCOUNT_UPDATED,
    EVENT_TOPIC_ACCOUNT_CREATED,
    EVENT_TOPIC_ACCOUNT_DELETED,
    EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
    EVENT_TOPIC_PUSH_CONNECTED,
    EVENT_TOPIC_PUSH_DISCONNECTED,
    EVENT_TOPIC_OPERATION_SETTLED,
    EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN,
    EVENT_TOPIC_RULE_FIRED,
    EVENT_TOPIC_RULE_DELIVERY_FAILED,
];

mod account_overview;
mod account_settings;
mod appearance;
mod automation;
mod commands;
mod conversations;
mod errors;
mod mail_query;
mod messages;
mod notifications;
mod outbox;
mod query_schema;
mod records;
mod rev_log;
mod rules;
mod smart_mailboxes;
mod sync;
mod unsubscribe;

pub use account_overview::*;
pub use account_settings::*;
pub use appearance::*;
pub use automation::*;
pub use commands::*;
pub use conversations::*;
pub use errors::*;
pub use mail_query::*;
pub use messages::*;
pub use notifications::*;
pub use outbox::*;
pub use query_schema::*;
pub use records::*;
pub use rev_log::*;
pub use rules::*;
pub use smart_mailboxes::*;
pub use sync::*;
pub use unsubscribe::*;

#[cfg(test)]
mod tests;
