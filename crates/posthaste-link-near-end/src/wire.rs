//! The per-seam wire profile the engine is instantiated over (D40: "one engine,
//! instantiated per seam").
//!
//! The engine owns everything seam-independent — the reconnect loop, the resume
//! cursor, deadlines, jittered backoff, permanent-vs-transient classification,
//! the reconciler. What *differs* between the client↔runtime link and the
//! runtime↔authority-server link is only the wire shape: whether a connection is
//! prepared first (a link open), how the forward/stream/settlement requests
//! are built, and what a frame parses into. [`Wire`] captures exactly that
//! seam-shaped remainder; [`RuntimeLinkWire`] is the client↔runtime profile
//! (used by the browser via wasm), and the runtime crate implements the
//! authority-server profile natively (its frame type is native-only, so the
//! impl lives runtime-side — this crate stays wasm-pure).

use posthaste_contract_core::{MutationRequest, RuntimeFrame, RuntimeLinkConnection};

use crate::engine::EngineError;
use crate::transport::{GetRequest, PostRequest, StreamRequest};

/// The outcome of parsing one down-stream payload (D49). A seam's wire either
/// yields a stamped data frame carrying its resume seq, or a `Reset` control
/// element telling the engine the far-end could not serve the resume point — the
/// near node must collapse-and-reseed and adopt `highest_seq` as its cursor.
pub enum ParsedFrame<Frame> {
    /// A data frame stamped with its resume seq.
    Frame { seq: u64, frame: Frame },
    /// A reset control element: collapse-and-reseed, adopt `highest_seq`.
    Reset { highest_seq: u64 },
}

/// A seam's wire profile: request shapes + frame parse. Everything here is
/// policy-free — cursor placement, retries, deadlines and classification stay
/// in the engine.
pub trait Wire {
    /// The typed frame this seam's down-stream carries.
    type Frame: 'static;

    /// The prepare POST the wire needs before subscribing (e.g. the client
    /// seam's link open), or `None` when the wire subscribes directly.
    fn prepare_request(&self) -> Option<PostRequest>;

    /// Digest a successful prepare response into the connection token
    /// (e.g. the link id) later requests carry. `Err` is permanent — the
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

    /// Parse one raw stream payload into a [`ParsedFrame`] — a stamped data frame
    /// (its seq drives the engine's resume cursor + gap detection) or a `Reset`
    /// control element (D49). `Err` is a malformed payload (dropped, counted).
    fn parse_frame(&self, data: &str) -> Result<ParsedFrame<Self::Frame>, String>;

    /// The settlement-query GET for a sent-but-unsettled record (D44b), or
    /// `None` when this seam has no cross-link settlement query (the
    /// reconciler then skips that step).
    fn settlement_request(
        &self,
        link_id: &str,
        client_mutation_id: &str,
    ) -> Option<GetRequest>;
}

/// The client↔runtime wire profile: link-prepared, `RuntimeFrame` down,
/// forwards stamped with the link id. `base_url` is a path prefix the host's
/// IO shim resolves against the API origin.
#[derive(Clone, Debug, Default)]
pub struct RuntimeLinkWire {
    /// Path prefix for every runtime route (e.g. `/v1`). The host resolves the
    /// origin + auth; the wire only builds the protocol path.
    pub base_url: String,
    /// Whether the link opts into incremental mail-list deltas.
    pub view_delta: bool,
    /// Optional account scope for a source-scoped link (`?sourceId=`).
    pub source_id: Option<String>,
}

impl Wire for RuntimeLinkWire {
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
        let link: RuntimeLinkConnection =
            serde_json::from_str(body).map_err(|e| format!("parse link: {e}"))?;
        Ok(link.link_id.as_str().to_string())
    }

    fn forward_request(
        &self,
        token: Option<&str>,
        request: &MutationRequest,
    ) -> Result<PostRequest, EngineError> {
        let link_id =
            token.ok_or_else(|| EngineError::transient("forward before link open"))?;
        // Stamp the engine-held link onto the typed request (parse in,
        // serialize out — the mutation crosses the wire as a validated
        // `MailOperation`, never a raw cast).
        let mut request = request.clone();
        request.link_id = Some(posthaste_contract_core::RuntimeLinkId::new(link_id));
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
                link_id,
                query_string(&query)
            ),
            headers: json_headers(),
            body,
        })
    }

    fn stream_request(&self, token: Option<&str>, cursor: Option<u64>) -> StreamRequest {
        let link = token.unwrap_or_default();
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
                link,
                query_string(&query)
            ),
            headers: Vec::new(),
        }
    }

    fn parse_frame(&self, data: &str) -> Result<ParsedFrame<RuntimeFrame>, String> {
        // The client↔runtime link stream carries `RuntimeFrame` (its seq rides
        // inside as `linkSeq`); it has no `Reset` control element — the runtime
        // far-end's collapse re-serves whole `ViewSnapshot`s, and a detected gap
        // resubscribes into that re-serve, so the reset is surfaced by the engine's
        // gap detection rather than a wire element.
        let frame: RuntimeFrame = serde_json::from_str(data).map_err(|e| e.to_string())?;
        Ok(ParsedFrame::Frame {
            seq: frame.link_seq().get(),
            frame,
        })
    }

    fn settlement_request(
        &self,
        link_id: &str,
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
                link_id,
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
