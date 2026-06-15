use super::*;

fn account_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::NotFound,
        "account not found",
    )
}

pub(super) fn load_account(
    state: &AppState,
    account_id: &AccountId,
) -> Result<AccountSettings, ApiError> {
    state
        .service
        .get_source(account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(account_not_found)
}
