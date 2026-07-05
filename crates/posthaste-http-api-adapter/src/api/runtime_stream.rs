use super::*;
use posthaste_contract_core::{
    MutationReceipt, MutationRequest, RuntimeError, RuntimeFrame, RuntimeLinkConnection,
    RuntimeLinkId, RuntimeLinkSeq, RuntimeMutationSettlement, ViewDescriptor, ViewId, ViewSnapshot,
};

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLinkQuery {
    pub source_id: Option<String>,
    /// The link can apply incremental mail-list deltas
    /// ([replication client-link L1](../../../docs/replication/client-link/L1.md)); when `true` the
    /// runtime sends `ViewDelta` frames instead of whole `ViewReplace`s.
    #[serde(default)]
    pub view_delta: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLinkStreamQuery {
    pub after_seq: Option<u64>,
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeLinkViewRequest {
    #[schema(value_type = Object)]
    pub descriptor: ViewDescriptor,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeLinkViewResponse {
    #[schema(value_type = String)]
    pub view_id: ViewId,
    pub snapshot: ViewSnapshot,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtendRuntimeLinkViewRequest {
    /// Number of additional rows to grow the view's window by.
    pub count: usize,
}

pub(crate) mod links;
pub(crate) mod mutations;
pub(crate) mod views;

pub use links::{close_runtime_link, open_runtime_link, stream_runtime_link};
pub use mutations::{run_runtime_link_mutation, runtime_link_mutation_settlement};
pub use views::{close_runtime_link_view, extend_runtime_link_view, open_runtime_link_view};

fn runtime_caller(source_id: Option<&str>) -> RuntimeCaller {
    let mut caller = RuntimeCaller::api();
    caller.account_scope = source_id.map(|source_id| vec![source_id.to_string()]);
    caller
}

fn frame_to_sse(frame: RuntimeFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(frame.link_seq().get().to_string())
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}
