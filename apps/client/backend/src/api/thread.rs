//! The thread family.

use posthaste_client_models::ThreadQuery;
use posthaste_domain_model::ThreadView;

use super::{offload_read, ApiFailure};
use crate::AppState;

pub(crate) async fn evaluate_thread(
    app: &AppState,
    query: ThreadQuery,
) -> Result<ThreadView, ApiFailure> {
    let service = app.service.clone();
    offload_read(move || Ok(service.get_thread(&query.account_id, &query.thread_id)?)).await
}
