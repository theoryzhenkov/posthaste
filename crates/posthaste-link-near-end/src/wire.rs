//! The per-seam wire profile the engine is instantiated over (D40: "one engine,
//! instantiated per seam").
//!
//! The engine owns everything seam-independent — the reconnect loop, the resume
//! cursor, deadlines, jittered backoff, permanent-vs-transient classification,
//! the reconciler. What *differs* between the client↔runtime link and the
//! runtime↔authority-server link is only the wire shape: whether a connection is
//! prepared first (a session open), how the forward/stream/settlement requests
//! are built, and what a frame parses into. [`Wire`] captures exactly that
//! seam-shaped remainder; [`RuntimeSessionWire`] is the client↔runtime profile
//! (used by the browser via wasm), and the runtime crate implements the
//! authority-server profile natively (its frame type is native-only, so the
//! impl lives runtime-side — this crate stays wasm-pure).

use posthaste_contract_core::{MutationRequest, RuntimeFrame, RuntimeSession};

use crate::engine::EngineError;
use crate::transport::{GetRequest, PostRequest, StreamRequest};

/// A seam's wire profile: request shapes + frame parse. Everything here is
/// policy-free — cursor placement, retries, deadlines and classification stay
/// in the engine.
pub trait Wire {
    /// The typed frame this seam's down-stream carries.
    type Frame: 'static;

    /// The prepare POST the wire needs before subscribing (e.g. the client
    /// seam's session open), or `None` when the wire subscribes directly.
    fn prepare_request(&self) -> Option<PostRequest>;

    /// Digest a successful prepare response into the connection token
    /// (e.g. the session id) later requests carry. `Err` is permanent — the
    /// contract itself is broken.
    fn parse_prepared(&self, body: &str) -> Result<String, String>;

    /// Build the forward POST for `request`. `token` is the prepare result
    /// (`None` when the wire has no prepare step or it has not run).
    fn forward_request(
        &self,
        token: Option<&str>,
        request: &MutationRequest,
    ) -> Result<PostRequest, EngineError>;

    /// Build the frame-stream request. `cursor` is the engine-owned resume
    /// cursor (`None` on a fresh subscribe).
    fn stream_request(&self, token: Option<&str>, cursor: Option<u64>) -> StreamRequest;

    /// Parse one raw stream payload into `(seq, frame)`. The seq drives the
    /// engine's resume cursor.
    fn parse_frame(&self, data: &str) -> Result<(u64, Self::Frame), String>;

    /// The settlement-query GET for a sent-but-unsettled record (D44b), or
    /// `None` when this seam has no cross-session settlement query (the
    /// reconciler then skips that step).
    fn settlement_request(
        &self,
        session_id: &str,
        client_mutation_id: &str,
    ) -> Option<GetRequest>;
}

/// The client↔runtime wire profile: session-prepared, `RuntimeFrame` down,
/// forwards stamped with the session id. `base_url` is a path prefix the host's
/// IO shim resolves against the API origin.
#[derive(Clone, Debug, Default)]
pub struct RuntimeSessionWire {
    /// Path prefix for every runtime route (e.g. `/v1`). The host resolves the
    /// origin + auth; the wire only builds the protocol path.
    pub base_url: String,
    /// Whether the session opts into incremental mail-list deltas.
    pub view_delta: bool,
    /// Optional account scope for a source-scoped session (`?sourceId=`).
    pub source_id: Option<String>,
}

impl Wire for RuntimeSessionWire {
    type Frame = RuntimeFrame;

    fn prepare_request(&self) -> Option<PostRequest> {
        let mut query = Vec::new();
        if self.view_delta {
            query.push("viewDelta=true".to_string());
        }
        if let Some(source) = &self.source_id {
            query.push(format!("sourceId={source}"));
        }
        Some(PostRequest {
            url: format!("{}/runtime/sessions{}", self.base_url, query_string(&query)),
            headers: json_headers(),
            body: String::new(),
        })
    }

    fn parse_prepared(&self, body: &str) -> Result<String, String> {
        let session: RuntimeSession =
            serde_json::from_str(body).map_err(|e| format!("parse session: {e}"))?;
        Ok(session.session_id.as_str().to_string())
    }

    fn forward_request(
        &self,
        token: Option<&str>,
        request: &MutationRequest,
    ) -> Result<PostRequest, EngineError> {
        let session_id =
            token.ok_or_else(|| EngineError::transient("forward before session open"))?;
        // Stamp the engine-held session onto the typed request (parse in,
        // serialize out — the mutation crosses the wire as a validated
        // `MailOperation`, never a raw cast).
        let mut request = request.clone();
        request.session_id = Some(posthaste_contract_core::RuntimeSessionId::new(session_id));
        let body = serde_json::to_string(&request)
            .map_err(|e| EngineError::permanent(format!("serialize mutation: {e}")))?;
        let mut query = Vec::new();
        if let Some(source) = &self.source_id {
            query.push(format!("sourceId={source}"));
        }
        Ok(PostRequest {
            url: format!(
                "{}/runtime/sessions/{}/mutations{}",
                self.base_url,
                session_id,
                query_string(&query)
            ),
            headers: json_headers(),
            body,
        })
    }

    fn stream_request(&self, token: Option<&str>, cursor: Option<u64>) -> StreamRequest {
        let session = token.unwrap_or_default();
        let mut query = Vec::new();
        if let Some(cursor) = cursor {
            query.push(format!("afterSeq={cursor}"));
        }
        if let Some(source) = &self.source_id {
            query.push(format!("sourceId={source}"));
        }
        StreamRequest {
            url: format!(
                "{}/runtime/sessions/{}/stream{}",
                self.base_url,
                session,
                query_string(&query)
            ),
            headers: Vec::new(),
        }
    }

    fn parse_frame(&self, data: &str) -> Result<(u64, RuntimeFrame), String> {
        let frame: RuntimeFrame = serde_json::from_str(data).map_err(|e| e.to_string())?;
        Ok((frame.session_seq().get(), frame))
    }

    fn settlement_request(
        &self,
        session_id: &str,
        client_mutation_id: &str,
    ) -> Option<GetRequest> {
        let mut query = Vec::new();
        if let Some(source) = &self.source_id {
            query.push(format!("sourceId={source}"));
        }
        Some(GetRequest {
            url: format!(
                "{}/runtime/sessions/{}/mutations/{}{}",
                self.base_url,
                session_id,
                client_mutation_id,
                query_string(&query)
            ),
            headers: Vec::new(),
        })
    }
}

pub(crate) fn json_headers() -> Vec<(String, String)> {
    vec![("content-type".to_string(), "application/json".to_string())]
}

pub(crate) fn query_string(parts: &[String]) -> String {
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}
