use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use std::time::Instant;

use futures_util::{future::pending, StreamExt};
use posthaste_domain_model::{AccountDriver, AccountId, AccountRuntimeOverview, AccountSettings, AccountStatus, CacheMaintenanceFeedback, CacheResourcePolicy, DomainEvent, GatewayError, Id, Identity, ProviderAuthKind, PushNotification, PushStatus, RemoteIdleScope, RemoteObservationPolicy, ServiceError, ServiceErrorKind, SyncMode, SyncProgress, SyncProgressStage, SyncTrigger, EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_PUSH_CONNECTED, EVENT_TOPIC_PUSH_DISCONNECTED};
use posthaste_domain_service::{CacheResourceGovernor, MailService, MailStore, PushEventStream, PushStreamEvent, ResilientPushConfig, SecretResolver, SecretStore, SharedGateway, StaticSecretResolver, SyncProgressReporter};
use posthaste_engine::{connect_jmap_client, LiveJmapGateway, MockJmapGateway};
use posthaste_call_policy::BackoffSchedule as BackoffPolicy;
use posthaste_imap::{
    ImapAdapterError, ImapConnectionConfig, LiveImapSmtpGateway, SmtpConnectionConfig,
};
use posthaste_observability::{events, ph_debug, ph_error, ph_info, ph_warn};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock, Semaphore};
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::sync::CancellationToken;
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
use manager::jitter_unit;
use runtime::run_account_runtime;
use sync_flow::{process_automation_backfill_batch, process_sync_trigger, sync_poll_interval};
use types::*;

#[cfg(test)]
use manager::{run_watchdog, SpawnIncarnation, WatchdogPolicy};
#[cfg(test)]
use runtime::{
    handle_push_event, handle_snooze_tick, process_sync_trigger_with_state, SyncTriggerRequest,
};
#[cfg(test)]
use sync_flow::sync_failure_stage;

#[cfg(test)]
mod tests;
