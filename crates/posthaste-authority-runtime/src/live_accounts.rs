use async_trait::async_trait;
use posthaste_domain_service::{
    AccountId, GatewayError, ServiceError, SharedGateway, SyncMode, SyncTrigger,
};

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
    fn account_count(&self) -> Option<usize> {
        None
    }

    async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError>;

    async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        mode: SyncMode,
    ) -> Result<usize, ServiceError>;

    async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<(), ServiceError>;
}

pub struct UnavailableLiveAccountRuntimeProvider;

#[async_trait]
impl LiveAccountRuntimeProvider for AccountSupervisor {
    fn account_count(&self) -> Option<usize> {
        Some(AccountSupervisor::account_count(self))
    }

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

    async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<(), ServiceError> {
        AccountSupervisor::trigger_account_sync(self, account_id, trigger).await
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

    async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        _trigger: SyncTrigger,
    ) -> Result<(), ServiceError> {
        Err(ServiceError::from(GatewayError::Unavailable(
            account_id.to_string(),
        )))
    }
}
