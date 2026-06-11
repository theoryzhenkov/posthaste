use super::*;

/// Find a free account id from `seed`, appending `-2`, `-3`, … on collision.
pub(super) fn allocate_unique_account_id(
    state: &AppState,
    seed: &str,
) -> Result<AccountId, ApiError> {
    let mut candidate = AccountId::from(seed);
    let mut suffix = 2;
    while state
        .service
        .get_source(&candidate)
        .map_err(ApiError::from_service_error)?
        .is_some()
    {
        candidate = AccountId::from(format!("{seed}-{suffix}"));
        suffix += 1;
    }
    Ok(candidate)
}

/// Persist a freshly-built account: save → start runtime → publish event.
///
/// If `save_source` fails after a secret was written to the keyring, roll the
/// secret back so a failed create does not orphan it (consistent across the
/// manual and OAuth creation paths). `delete_managed_secret` no-ops unless the
/// account carries an OS-managed secret.
pub(super) async fn persist_new_account(
    state: &Arc<AppState>,
    account: &AccountSettings,
    topic: &str,
) -> Result<(), ApiError> {
    if let Err(error) = state.service.save_source(account) {
        delete_managed_secret(state, account.transport.secret_ref.as_ref())?;
        return Err(ApiError::from_service_error(error));
    }
    state.supervisor.start_account(account).await;
    append_and_publish_account_event(state, &account.id, topic).map_err(store_error_to_api)?;
    Ok(())
}
