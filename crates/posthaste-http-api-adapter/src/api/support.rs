use super::*;

fn account_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::NotFound,
        "account not found",
    )
}

pub(super) async fn ensure_account_exists(
    state: &AppState,
    account_id: &AccountId,
) -> Result<(), ApiError> {
    state
        .runtime
        .get_account(RuntimeCaller::api(), account_id.clone())
        .await
        .map(|_| ())
        .map_err(|error| {
            let error = ApiError::from_runtime_error(error);
            if error.body.code == ApiErrorCode::NotFound {
                account_not_found()
            } else {
                error
            }
        })
}
