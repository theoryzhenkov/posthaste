//! Migration-residue test harness: build a runtime handle around a pre-existing
//! service/store graph.
//!
//! This was an `AppState` method during the api-runtime-wrapper migration, then
//! a free function in `posthaste-server/src/migration.rs`. The wrapper migration
//! is complete (RFC D20); the live server integration test suites that stand up
//! a runtime around a pre-configured service graph still need this entry point,
//! so it lives here in the testkit (its only consumers). It delegates to
//! `posthaste_authority_runtime`'s public
//! `from_api_bridge_with_account_supervisor_for_migration` constructor.

use std::sync::Arc;

use tokio::sync::broadcast;

use posthaste_authority_runtime::{AccountSupervisor, AuthorityRuntimeApiMigrationBridge};
use posthaste_domain_service::{DomainEvent, MailService, MailStore, SecretStore};
use posthaste_runtime::RuntimeHandle;

/// Build a runtime handle around an existing service/store/secret-store/event
/// graph, with the given account supervisor as the live-account provider.
///
/// `service.list_sources()` is read to size the runtime's account count — the
/// supervisor must already cover those accounts.
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
