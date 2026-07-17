//! `GET /blobs/{blob_id}`: immutable binary resources.

use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use posthaste_domain_model::BlobId;

use super::{offload_read, ApiFailure, ApiState};

/// `GET /blobs/{blob_id}`: serve an attachment blob through the owning
/// account's gateway (cached raw bytes short-circuit the provider call
/// inside the service). Blobs are immutable, so the response carries
/// long-lived caching headers.
pub(crate) async fn handle_blob_download(
    State(state): State<ApiState>,
    Path(blob_id): Path<String>,
) -> Result<Response, ApiFailure> {
    let blob = BlobId::from(blob_id.as_str());
    let store = state.app.database_store.clone();
    let lookup = blob.clone();
    let Some((account_id, message_id, attachment)) =
        offload_read(move || Ok(store.find_attachment_by_blob(&lookup)?)).await?
    else {
        return Err(ApiFailure::unknown_id(format!("blob {blob_id}")));
    };
    let gateway = state
        .app
        .supervisor
        .gateway(&account_id)
        .await
        .map_err(|_| {
            ApiFailure::unavailable(format!(
                "account {} is not connected; the attachment cannot be fetched right now",
                account_id.as_str()
            ))
        })?;
    let bytes = state
        .app
        .service
        .download_blob(&account_id, &message_id, &blob, gateway.as_ref())
        .await?;
    Ok(blob_response(bytes, &attachment.mime_type))
}

fn blob_response(bytes: Vec<u8>, mime_type: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, mime_type.to_string()),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}
