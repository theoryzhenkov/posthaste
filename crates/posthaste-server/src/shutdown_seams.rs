//! The composition root's teardown seams (D60/M20): bridge the near
//! `posthaste-http-api-adapter` [`SupervisorStop`]/[`StoreClose`] seam traits to
//! the concrete far-node/store types this crate owns. The adapter defines the
//! ordered [`ShutdownSequence`](posthaste_http_api_adapter::ShutdownSequence)
//! without depending on the far-node or store crates; these impls are where the
//! real components attach.

use std::sync::Arc;

use async_trait::async_trait;
use posthaste_authority_server::AccountSupervisor;
use posthaste_http_api_adapter::{StoreClose, SupervisorStop, SUPERVISOR_STOP_DEADLINE};
use posthaste_store::DatabaseStore;

/// Teardown step (b): stop the in-process account supervisor. M21 replaces the
/// supervisor internals behind `stop_all`; this seam is unchanged by it.
pub(crate) struct AccountSupervisorStop(pub Arc<AccountSupervisor>);

#[async_trait]
impl SupervisorStop for AccountSupervisorStop {
    async fn stop_all(&self) {
        // The sequence bounds this phase too, but pass the phase budget so the
        // supervisor's own cooperative-join / abort-escalation runs inside it
        // (M21): stragglers are aborted rather than abandoned by an outer cut.
        self.0.stop_all(SUPERVISOR_STOP_DEADLINE).await;
    }
}

/// Teardown step (c): close the SQLite store. The WAL checkpoint lands in M22,
/// inside `DatabaseStore::close`.
pub(crate) struct DatabaseStoreClose(pub Arc<DatabaseStore>);

#[async_trait]
impl StoreClose for DatabaseStoreClose {
    async fn close(&self) {
        self.0.close();
    }
}
