use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{future::pending, StreamExt};
use mail_domain::{
    AccountDriver, AccountId, AccountRuntimeOverview, AccountSettings, AccountStatus, DomainEvent,
    GatewayError, Identity, MailService, MailStore, PushEventStream, PushStatus,
    PushStreamEvent, ResilientPushConfig, SecretStore, ServiceError, SharedGateway, SyncTrigger,
    EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_PUSH_CONNECTED, EVENT_TOPIC_PUSH_DISCONNECTED,
};
use mail_jmap::{connect_jmap_client, LiveJmapGateway, MockJmapGateway};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tokio::task::JoinHandle;

use crate::push::resilient_push_stream;

pub struct AccountSupervisor {
    shared: Arc<SupervisorShared>,
    runtimes: RwLock<HashMap<String, ManagedRuntime>>,
}

struct SupervisorShared {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    runtime_overviews: RwLock<HashMap<String, AccountRuntimeOverview>>,
    poll_interval: Duration,
}

struct ManagedRuntime {
    command_tx: mpsc::Sender<RuntimeCommand>,
    handle: JoinHandle<()>,
}

enum RuntimeCommand {
    Trigger {
        trigger: SyncTrigger,
        reply: oneshot::Sender<Result<usize, ServiceError>>,
    },
}

pub struct AccountVerification {
    pub ok: bool,
    pub identity: Option<Identity>,
    pub push_supported: bool,
}

struct AccountConnection {
    gateway: SharedGateway,
    push_events: Option<PushEventStream>,
}

impl AccountSupervisor {
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
        poll_interval: Duration,
    ) -> Self {
        Self {
            shared: Arc::new(SupervisorShared {
                service,
                store,
                secret_store,
                event_sender,
                runtime_overviews: RwLock::new(HashMap::new()),
                poll_interval,
            }),
            runtimes: RwLock::new(HashMap::new()),
        }
    }

    pub async fn start_account(&self, account: &AccountSettings) {
        self.stop_account(&account.id).await;
        if !account.enabled {
            self.shared
                .set_runtime_overview(
                    &account.id,
                    AccountRuntimeOverview {
                        status: AccountStatus::Disabled,
                        push: PushStatus::Disabled,
                        ..Default::default()
                    },
                )
                .await;
            return;
        }

        let (command_tx, command_rx) = mpsc::channel(32);
        let shared = self.shared.clone();
        let account = account.clone();
        let account_id = account.id.clone();
        let handle = tokio::spawn(async move {
            run_account_runtime(shared, account, command_rx).await;
        });
        self.runtimes.write().await.insert(
            account_id.to_string(),
            ManagedRuntime { command_tx, handle },
        );
    }

    pub async fn stop_account(&self, account_id: &AccountId) {
        if let Some(runtime) = self.runtimes.write().await.remove(account_id.as_str()) {
            runtime.handle.abort();
        }
        self.shared.service.remove_gateway(account_id);
    }

    pub async fn remove_account(&self, account_id: &AccountId) {
        self.stop_account(account_id).await;
        self.shared
            .runtime_overviews
            .write()
            .await
            .remove(account_id.as_str());
    }

    pub async fn sync_account(&self, account_id: &AccountId) -> Result<usize, ServiceError> {
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime
            .command_tx
            .send(RuntimeCommand::Trigger {
                trigger: SyncTrigger::Manual,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::from(GatewayError::Unavailable(account_id.to_string())))?
    }

    pub async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.shared.runtime_overview(account_id).await
    }

    pub async fn verify_account(
        &self,
        account: &AccountSettings,
    ) -> Result<AccountVerification, ServiceError> {
        let conn = build_connection(account, self.shared.secret_store.as_ref()).await?;
        let identity = conn.gateway.fetch_identity(&account.id).await.ok();
        Ok(AccountVerification {
            ok: true,
            identity,
            push_supported: matches!(account.driver, AccountDriver::Jmap),
        })
    }
}

async fn run_account_runtime(
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
    mut command_rx: mpsc::Receiver<RuntimeCommand>,
) {
    let account_id = account.id.clone();
    let mut connection: Option<AccountConnection> = None;
    let mut interval = tokio::time::interval(shared.poll_interval);

    shared
        .set_runtime_overview(
            &account_id,
            AccountRuntimeOverview {
                status: AccountStatus::Offline,
                push: PushStatus::Reconnecting,
                ..Default::default()
            },
        )
        .await;

    // Initial sync + gateway setup
    let _ = process_sync_trigger(&shared, &account, SyncTrigger::Startup, &mut connection, None)
        .await;

    loop {
        let next_push = async {
            match connection.as_mut().and_then(|c| c.push_events.as_mut()) {
                Some(stream) => stream.next().await,
                None => pending().await,
            }
        };

        tokio::select! {
            _ = interval.tick() => {
                let _ = process_sync_trigger(
                    &shared, &account, SyncTrigger::Poll, &mut connection, None,
                ).await;
            }
            Some(command) = command_rx.recv() => {
                match command {
                    RuntimeCommand::Trigger { trigger, reply } => {
                        let _ = process_sync_trigger(
                            &shared, &account, trigger, &mut connection, Some(reply),
                        ).await;
                    }
                }
            }
            Some(event) = next_push => {
                match event {
                    PushStreamEvent::Notification(_) => {
                        let _ = process_sync_trigger(
                            &shared, &account, SyncTrigger::Push, &mut connection, None,
                        ).await;
                    }
                    PushStreamEvent::Connected { transport } => {
                        shared.set_push_status(&account_id, PushStatus::Connected).await;
                        tracing_connected(&shared, &account_id, transport);
                    }
                    PushStreamEvent::Disconnected { transport, reason } => {
                        shared.handle_push_disconnect(&account_id, &format!("{transport}: {reason}")).await;
                    }
                    PushStreamEvent::Fallback { from, to } => {
                        shared.handle_push_disconnect(
                            &account_id,
                            &format!("falling back from {from} to {to}"),
                        ).await;
                    }
                }
            }
        }
    }
}

fn tracing_connected(shared: &SupervisorShared, account_id: &AccountId, transport: &str) {
    if let Ok(event) = shared.store.append_event(
        account_id,
        EVENT_TOPIC_PUSH_CONNECTED,
        None,
        None,
        json!({ "transport": transport }),
    ) {
        shared.publish_events(&[event]);
    }
}

async fn process_sync_trigger(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    trigger: SyncTrigger,
    connection: &mut Option<AccountConnection>,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
) -> Result<(), ServiceError> {
    let account_id = account.id.clone();
    shared
        .set_status_only(&account_id, AccountStatus::Syncing)
        .await;

    let result = match ensure_connection(shared, account, connection).await {
        Ok(()) => {
            shared
                .service
                .sync_account(&account_id, trigger.clone())
                .await
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(events) => {
            let event_count = events.len();
            shared.publish_events(&events);
            shared.mark_sync_success(&account_id).await;
            if let Some(reply) = reply {
                let _ = reply.send(Ok(event_count));
            }
        }
        Err(error) => {
            shared.service.remove_gateway(&account_id);
            *connection = None; // tears down gateway + push stream together
            let stage = if matches!(
                error,
                ServiceError::Gateway(GatewayError::Unavailable(_))
                    | ServiceError::Gateway(GatewayError::Auth)
                    | ServiceError::Gateway(GatewayError::Network(_))
            ) {
                "connect"
            } else {
                "sync"
            };
            if let Ok(event) = shared.service.record_sync_failure(
                &account_id,
                error.code(),
                &error.to_string(),
                trigger,
                stage,
            ) {
                shared.publish_events(&[event]);
            }
            shared.mark_sync_failure(&account_id, &error).await;
            if let Some(reply) = reply {
                let _ = reply.send(Err(error));
            }
        }
    }

    Ok(())
}

async fn ensure_connection(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    connection: &mut Option<AccountConnection>,
) -> Result<(), ServiceError> {
    if connection.is_some() {
        return Ok(());
    }
    let conn = build_connection(account, shared.secret_store.as_ref()).await?;
    shared.service.set_gateway(&account.id, conn.gateway.clone());
    *connection = Some(conn);
    Ok(())
}

async fn build_connection(
    account: &AccountSettings,
    secret_store: &dyn SecretStore,
) -> Result<AccountConnection, ServiceError> {
    match account.driver {
        AccountDriver::Mock => Ok(AccountConnection {
            gateway: Arc::new(MockJmapGateway::default()),
            push_events: None,
        }),
        AccountDriver::Jmap => {
            let url = account
                .transport
                .base_url
                .as_deref()
                .ok_or_else(|| GatewayError::Rejected("missing JMAP base URL".to_string()))?;
            let username = account
                .transport
                .username
                .as_deref()
                .ok_or_else(|| GatewayError::Rejected("missing JMAP username".to_string()))?;
            let secret_ref = account.transport.secret_ref.as_ref().ok_or_else(|| {
                GatewayError::Rejected("missing JMAP secret reference".to_string())
            })?;
            let password = secret_store.resolve(secret_ref)?;
            let client = connect_jmap_client(url, username, &password).await?;
            let gateway: SharedGateway = Arc::new(LiveJmapGateway::from_client(client));

            let transports = gateway.push_transports();
            let mut transports = transports.into_iter();
            let primary = transports.next();
            let fallback = transports.next();

            let push_events = primary.map(|primary| {
                resilient_push_stream(
                    account.id.clone(),
                    primary,
                    fallback,
                    ResilientPushConfig::default(),
                )
            });

            Ok(AccountConnection {
                gateway,
                push_events,
            })
        }
    }
}

impl SupervisorShared {
    fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.runtime_overviews
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .unwrap_or_default()
    }

    async fn set_status_only(&self, account_id: &AccountId, status: AccountStatus) {
        let mut current = self.runtime_overview(account_id).await;
        current.status = status;
        self.set_runtime_overview(account_id, current).await;
    }

    async fn set_push_status(&self, account_id: &AccountId, push: PushStatus) {
        let mut current = self.runtime_overview(account_id).await;
        current.push = push;
        self.set_runtime_overview(account_id, current).await;
    }

    async fn mark_sync_success(&self, account_id: &AccountId) {
        let mut current = self.runtime_overview(account_id).await;
        current.status = AccountStatus::Ready;
        current.last_sync_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        current.last_sync_error = None;
        current.last_sync_error_code = None;
        if matches!(current.push, PushStatus::Disabled) {
            current.push = PushStatus::Reconnecting;
        }
        self.set_runtime_overview(account_id, current).await;
    }

    async fn mark_sync_failure(&self, account_id: &AccountId, error: &ServiceError) {
        let mut current = self.runtime_overview(account_id).await;
        current.status = match error {
            ServiceError::Gateway(GatewayError::Auth) => AccountStatus::AuthError,
            ServiceError::Gateway(GatewayError::Network(_))
            | ServiceError::Gateway(GatewayError::Unavailable(_))
            | ServiceError::Secret(_) => AccountStatus::Offline,
            _ => AccountStatus::Degraded,
        };
        current.last_sync_error = Some(error.to_string());
        current.last_sync_error_code = Some(error.code().to_string());
        if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
            current.push = PushStatus::Reconnecting;
        }
        self.set_runtime_overview(account_id, current).await;
    }

    async fn handle_push_disconnect(&self, account_id: &AccountId, message: &str) {
        let event = self
            .store
            .append_event(
                account_id,
                EVENT_TOPIC_PUSH_DISCONNECTED,
                None,
                None,
                json!({ "message": message }),
            )
            .ok();
        if let Some(event) = event {
            self.publish_events(&[event]);
        }
        self.set_push_status(account_id, PushStatus::Reconnecting)
            .await;
    }

    async fn set_runtime_overview(&self, account_id: &AccountId, overview: AccountRuntimeOverview) {
        let previous = self
            .runtime_overviews
            .write()
            .await
            .insert(account_id.to_string(), overview.clone());

        let mut side_effects = Vec::new();
        if previous.as_ref().map(|item| &item.status) != Some(&overview.status) {
            if let Ok(event) = self.store.append_event(
                account_id,
                EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
                None,
                None,
                json!({
                    "status": format!("{:?}", overview.status).to_lowercase(),
                    "push": format!("{:?}", overview.push).to_lowercase(),
                    "lastSyncAt": overview.last_sync_at,
                    "lastSyncError": overview.last_sync_error,
                    "lastSyncErrorCode": overview.last_sync_error_code,
                }),
            ) {
                side_effects.push(event);
            }
        }

        match (previous.as_ref().map(|item| &item.push), &overview.push) {
            (Some(PushStatus::Connected), PushStatus::Connected) => {}
            (_, PushStatus::Connected) => {
                if let Ok(event) = self.store.append_event(
                    account_id,
                    EVENT_TOPIC_PUSH_CONNECTED,
                    None,
                    None,
                    json!({}),
                ) {
                    side_effects.push(event);
                }
            }
            (Some(PushStatus::Connected), _) => {
                if let Ok(event) = self.store.append_event(
                    account_id,
                    EVENT_TOPIC_PUSH_DISCONNECTED,
                    None,
                    None,
                    json!({}),
                ) {
                    side_effects.push(event);
                }
            }
            _ => {}
        }

        self.publish_events(&side_effects);
    }
}
