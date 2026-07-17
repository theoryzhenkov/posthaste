//! The sync family: trigger one account's sync cycle on demand.

use posthaste_client_models::SyncNowIntent;

use super::ApiFailure;
use crate::AppState;

/// Kick the account's runtime into a sync cycle with the requested depth.
/// Acceptance-only, like every command: the cycle runs detached, and its
/// progress and outcome surface as account status and stream events, the
/// same as any scheduled sync.
pub(crate) fn sync_now(app: &AppState, intent: SyncNowIntent) -> Result<u64, ApiFailure> {
    let account = app
        .service
        .get_source(&intent.account_id)?
        .ok_or_else(|| ApiFailure::unknown_id(format!("account {}", intent.account_id.as_str())))?;
    if !account.enabled {
        return Err(ApiFailure::unavailable(format!(
            "account {} is disabled; enable it before syncing",
            account.id.as_str()
        )));
    }
    let mode = intent.mode.unwrap_or_default();
    let supervisor = app.supervisor.clone();
    let account_id = account.id;
    tokio::spawn(async move {
        // A cycle failure is already recorded by the runtime as a sync-failed
        // event and a degraded account status; nothing to add here.
        let _ = supervisor.sync_account_with_mode(&account_id, mode).await;
    });
    Ok(app.events.generation())
}
