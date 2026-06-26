//! MIGRATION(api-runtime-wrapper): build a runtime handle around existing
//! service/store/supervisor parts for test/compat harnesses, until all router
//! state is produced by the runtime builder. These were inherent `AppState`
//! methods; they moved here as free functions when `AppState` became a near
//! (store-free) type in `posthaste-api`.
//!
//! @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle

use std::sync::Arc;
use std::time::Duration;

use posthaste_authority_runtime::{
    AccountSupervisor, AuthorityRuntimeApiMigrationBridge, RuntimeHandle,
};
use posthaste_domain::{DomainEvent, MailService, MailStore, SecretStore};
use tokio::sync::broadcast;

pub fn runtime_handle_for_migration(
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
) -> RuntimeHandle {
    let account_runtime_provider = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        Duration::from_secs(60),
    ));
    runtime_handle_with_account_runtime_provider_for_migration(
        service,
        store,
        secret_store,
        event_sender,
        account_runtime_provider,
    )
}

pub fn runtime_handle_with_account_runtime_provider_for_migration(
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    account_runtime_provider: Arc<AccountSupervisor>,
) -> RuntimeHandle {
    let account_count = service
        .list_sources()
        .expect("migration runtime handle should read configured sources")
        .len();
    posthaste_authority_runtime::from_api_bridge_with_account_supervisor_for_migration(
        AuthorityRuntimeApiMigrationBridge::new(service, store, secret_store, event_sender),
        account_count,
        account_runtime_provider,
    )
    .handle
}
