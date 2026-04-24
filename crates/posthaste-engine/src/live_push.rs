use posthaste_domain::PushTransport;

use crate::live::LiveJmapGateway;

/// Return available push transports, preferring WebSocket over SSE.
///
/// @spec docs/L2-transport#pushtransport
pub(crate) fn push_transports(gateway: &LiveJmapGateway) -> Vec<Box<dyn PushTransport>> {
    let mut transports: Vec<Box<dyn PushTransport>> = Vec::new();
    if let Some(ws) = gateway.ws() {
        transports.push(Box::new(crate::push_ws::WsPushTransport::new(ws.clone())));
    }
    transports.push(Box::new(crate::push_sse::SsePushTransport::new(
        gateway.client().clone(),
    )));
    transports
}
