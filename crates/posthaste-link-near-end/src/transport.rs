//! The zero-policy IO surface (RFC-L2-architecture-cleanup §6.11 / D45).
//!
//! [`Transport`] is the *only* thing the engine needs from its host: a way to
//! POST a JSON body and a way to open a byte-frame stream. It carries **no**
//! policy — no deadlines, no retries, no cursor, no parse, no
//! permanent-vs-transient classification. All of that lives in the engine, once
//! ([`crate::engine`]). A browser host binds these two methods to `fetch` +
//! `fetchEventSource`; a native host binds them to `reqwest` + SSE. The engine
//! is identical across both.
//!
//! ## Object-safe, boxed — and why
//!
//! The methods return boxed **local** futures/streams (`LocalBoxFuture` /
//! `LocalBoxStream`, i.e. `?Send`), so the trait is object-safe and the engine
//! holds it as `Rc<dyn Transport>`. Two forces make this the right shape rather
//! than a generic `Transport` bound:
//!
//! 1. The engine is exported across the wasm-bindgen boundary as a single,
//!    non-generic handle constructed with JS callbacks — a generic engine would
//!    leak a type parameter the bindgen layer cannot name.
//! 2. Browser IO futures (`JsFuture`) are `!Send`; requiring `Send` would make
//!    the trait un-implementable there. `LocalBoxFuture`/`LocalBoxStream` drop
//!    the `Send` bound while staying `'static`, which is all the engine's
//!    single-threaded task needs.
//!
//! Native reuse (the runtime→authority-server near-end, a later unit) is
//! unaffected: a native `Transport` impl boxes `reqwest` futures the same way.

use futures_util::future::LocalBoxFuture;
use futures_util::stream::LocalBoxStream;

/// A JSON POST the engine asks the host to perform. `url` is a path relative to
/// the runtime API root (the host prepends origin + auth); the engine owns every
/// query parameter that is policy (e.g. the resume cursor is baked into
/// [`StreamRequest::url`], never here).
#[derive(Clone, Debug)]
pub struct PostRequest {
    pub url: String,
    /// Protocol headers the engine sets (e.g. `content-type`). The host adds
    /// auth/transport headers on top; this list never carries policy.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// The host's response to a [`PostRequest`]. The engine classifies `status`
/// (4xx-permanent vs transient) — the transport reports it raw.
#[derive(Clone, Debug)]
pub struct PostResponse {
    pub status: u16,
    pub body: String,
}

/// A transport-level failure with no HTTP status (connection refused, DNS,
/// abort, the host's own timeout). Always treated transient by the engine —
/// a permanent verdict only ever comes from a 4xx *status*.
#[derive(Clone, Debug)]
pub struct TransportError {
    pub message: String,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// A frame stream the engine asks the host to open. `url` already carries the
/// engine-owned resume cursor (`?afterSeq=`) — the host opens exactly this URL
/// and streams back [`StreamEvent`]s, nothing more.
#[derive(Clone, Debug)]
pub struct StreamRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// One event from an open frame stream. The transport reports these raw; the
/// engine parses `Message` into a typed frame at the boundary and classifies
/// `Error` into permanent-vs-transient. `status` is the HTTP status the host saw
/// (present on an open failure, `None` for a mid-stream drop).
#[derive(Clone, Debug)]
pub enum StreamEvent {
    /// The stream opened (headers received, 2xx). The engine resets its
    /// reconnect backoff and runs the reconciler here.
    Open,
    /// A raw frame payload line (one SSE `data:` value). The engine parses it.
    Message(String),
    /// The stream ended cleanly (server closed, EOF). The engine reconnects.
    Closed,
    /// The stream errored. The engine classifies: an HTTP 4xx `status` is
    /// permanent (stop); anything else is transient (reconnect with backoff).
    Error {
        status: Option<u16>,
        message: String,
    },
}

/// The zero-policy IO surface (see module docs). Object-safe: the engine holds
/// `Rc<dyn Transport>`.
pub trait Transport {
    /// POST a JSON body and resolve with the raw status + body. A network-level
    /// failure (no status) resolves `Err`.
    fn post_json(
        &self,
        request: PostRequest,
    ) -> LocalBoxFuture<'static, Result<PostResponse, TransportError>>;

    /// Open a frame stream. The returned stream yields [`StreamEvent`]s until it
    /// terminates; the engine owns whether/when to re-open it.
    fn open_stream(&self, request: StreamRequest) -> LocalBoxStream<'static, StreamEvent>;
}
