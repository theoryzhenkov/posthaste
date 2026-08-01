//! The maintenance family: user-requested local-store repairs.

use posthaste_client_models::RederiveMessageMetadataIntent;
use posthaste_observability::{events, ph_info};

use super::ApiFailure;
use crate::AppState;

/// Re-derive every cached message's metadata from its retained raw MIME.
///
/// Unlike the rest of this family's neighbours in the UI, this touches no
/// provider and destroys nothing: it re-reads `.eml` files already on disk and
/// fills only columns that are still empty. The deferred startup pass does the
/// same work automatically once per derivation revision; this is the escape
/// hatch for a user who ran that already and still sees blank fields.
///
/// Runs to completion before replying (the frontend shows a spinner): the
/// population is bounded by the body cache's byte budget, and a fire-and-forget
/// version would have nothing truthful to report.
pub(crate) async fn rederive_message_metadata(
    app: &AppState,
    _intent: RederiveMessageMetadataIntent,
) -> Result<u64, ApiFailure> {
    let store = app.database_store.clone();
    let report = tokio::task::spawn_blocking(move || store.rederive_message_metadata())
        .await
        .map_err(|error| ApiFailure::internal(format!("re-derive task failed: {error}")))??;
    ph_info!(
        events::STORE_METADATA_REDERIVE_REQUESTED,
        examined = report.examined,
        filled = report.filled,
        unreadable = report.unreadable,
        "manual message-metadata re-derive completed"
    );
    // Only a pass that actually wrote something invalidates what clients hold;
    // a no-op repair must not churn every open window's queries.
    if report.filled > 0 {
        return Ok(app.events.bump());
    }
    Ok(app.events.generation())
}
