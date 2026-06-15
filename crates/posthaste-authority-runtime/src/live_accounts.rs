use async_trait::async_trait;
use posthaste_domain::{AccountId, GatewayError, ServiceError, SharedGateway, SyncMode};

use crate::supervisor::AccountSupervisor;

/// Runtime-owned access to live account runtime resources.
///
/// This is the temporary seam for migrating account supervisor ownership into
/// `posthaste-authority-runtime`: HTTP adapters call the runtime handle, while
/// the concrete supervisor implementation can move behind this trait in small
/// slices.
///
/// spec: docs/backend/L3#supervisor-ownership-migration
#[async_trait]
pub trait LiveAccountRuntimeProvider: Send + Sync {
    async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError>;

    async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        mode: SyncMode,
    ) -> Result<usize, ServiceError>;
}

pub struct UnavailableLiveAccountRuntimeProvider;

#[async_trait]
impl LiveAccountRuntimeProvider for AccountSupervisor {
    async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError> {
        AccountSupervisor::gateway(self, account_id).await
    }

    async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        mode: SyncMode,
    ) -> Result<usize, ServiceError> {
        AccountSupervisor::sync_account_with_mode(self, account_id, mode).await
    }
}

#[async_trait]
impl LiveAccountRuntimeProvider for UnavailableLiveAccountRuntimeProvider {
    async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError> {
        Err(ServiceError::from(GatewayError::Unavailable(
            account_id.to_string(),
        )))
    }

    async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        _mode: SyncMode,
    ) -> Result<usize, ServiceError> {
        Err(ServiceError::from(GatewayError::Unavailable(
            account_id.to_string(),
        )))
    }
}
