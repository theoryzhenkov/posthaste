use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use futures_util::{future::pending, StreamExt};
use posthaste_domain::{
    AccountDriver, AccountId, AccountRuntimeOverview, AccountSettings, AccountStatus,
    CacheMaintenanceFeedback, CacheResourceGovernor, CacheResourcePolicy, DomainEvent,
    GatewayError, Id, Identity, MailService, MailStore, ProviderAuthKind, PushEventStream,
    PushNotification, PushStatus, PushStreamEvent, RemoteIdleScope, RemoteObservationPolicy,
    ResilientPushConfig, SecretStore, ServiceError, ServiceErrorKind, SharedGateway, SyncMode,
    SyncProgress, SyncProgressReporter, SyncProgressStage, SyncTrigger,
    EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_PUSH_CONNECTED, EVENT_TOPIC_PUSH_DISCONNECTED,
};
use posthaste_engine::{connect_jmap_client, LiveJmapGateway, MockJmapGateway};
use posthaste_imap::{
    imap_idle_event_stream, ImapAdapterError, ImapConnectionConfig, LiveImapSmtpGateway,
    SmtpConnectionConfig,
};
use posthaste_observability::{events, ph_debug, ph_error, ph_info, ph_warn};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use tracing::{info_span, Instrument};

use crate::oauth::{OAuthTokenService, OAuthTokenSet};
use crate::push::resilient_push_stream;

mod cache;
mod connection;
mod manager;
mod runtime;
mod shared;
mod sync_flow;
mod types;

pub use types::{AccountSupervisor, AccountVerification};

use cache::process_cache_maintenance_batch;
use connection::{build_connection, ensure_connection};
use runtime::run_account_runtime;
use sync_flow::{process_automation_backfill_batch, process_sync_trigger, sync_poll_interval};
use types::*;

#[cfg(test)]
use runtime::handle_push_event;
#[cfg(test)]
use sync_flow::sync_failure_stage;

#[cfg(test)]
mod tests;
