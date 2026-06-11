use super::*;

pub(crate) async fn live_gateway(
    state: &AppState,
    account_id: &AccountId,
) -> Result<SharedGateway, ApiError> {
    state
        .supervisor
        .gateway(account_id)
        .await
        .map_err(ApiError::from_service_error)
}

pub(crate) async fn optional_live_gateway(
    state: &AppState,
    account_id: &AccountId,
) -> Option<SharedGateway> {
    state.supervisor.gateway(account_id).await.ok()
}

pub(crate) fn require_live_gateway(
    gateway: Option<SharedGateway>,
    account_id: &AccountId,
) -> Result<SharedGateway, ApiError> {
    gateway.ok_or_else(|| {
        ApiError::from_service_error(ServiceError::from(GatewayError::Unavailable(
            account_id.to_string(),
        )))
    })
}
