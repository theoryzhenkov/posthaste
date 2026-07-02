use super::*;
use posthaste_contract_core::{
    MutationReceipt, MutationRequest, RuntimeError, RuntimeFrame, RuntimeSession, RuntimeSessionId,
    RuntimeSessionSeq, ViewDescriptor, ViewId, ViewSnapshot,
};

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionQuery {
    pub source_id: Option<String>,
    /// The session can apply incremental mail-list deltas
    /// ([replication client-link L1](../../../docs/replication/client-link/L1.md)); when `true` the
    /// runtime sends `ViewDelta` frames instead of whole `ViewReplace`s.
    #[serde(default)]
    pub view_delta: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionStreamQuery {
    pub after_seq: Option<u64>,
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeSessionViewRequest {
    #[schema(value_type = Object)]
    pub descriptor: ViewDescriptor,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeSessionViewResponse {
    #[schema(value_type = String)]
    pub view_id: ViewId,
    pub snapshot: ViewSnapshot,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtendRuntimeSessionViewRequest {
    /// Number of additional rows to grow the view's window by.
    pub count: usize,
}

pub(crate) mod mutations;
pub(crate) mod sessions;
pub(crate) mod views;

pub use mutations::run_runtime_session_mutation;
pub use sessions::{close_runtime_session, open_runtime_session, stream_runtime_session};
pub use views::{
    close_runtime_session_view, extend_runtime_session_view, open_runtime_session_view,
};

fn runtime_caller(source_id: Option<&str>) -> RuntimeCaller {
    let mut caller = RuntimeCaller::api();
    caller.account_scope = source_id.map(|source_id| vec![source_id.to_string()]);
    caller
}

fn frame_to_sse(frame: RuntimeFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(frame.session_seq().get().to_string())
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}
