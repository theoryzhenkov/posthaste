//! The link **near-end**: the consuming half of a coherent link plus its
//! resilience policy, extracted into one wasm-pure engine
//! (RFC-L2-architecture-cleanup D39/D40/D41/D44/D45).
//!
//! A near node holds a near-end to talk *up* a link — `forward_mutation` up,
//! frame `subscribe` down. Before this crate that half existed three times: the
//! runtime's `reqwest` transport (no deadline, no reconnect — lifecycle-debt
//! rows 1/2), the web client's `sessionClient.ts` (flat-1s retry, cursor dropped
//! on reconnect, unchecked frame cast — rows 8/9), and the in-process path. All
//! three were broken the same way. This crate is the single fix:
//!
//! * [`Transport`] — the zero-policy IO surface (post-json + open-stream) the
//!   host binds to `fetch`/`fetchEventSource` (browser) or `reqwest`/SSE (native);
//! * [`NearEnd`] — the engine: session lifecycle, a reconnect loop that owns the
//!   resume cursor, a request deadline + jittered capped backoff, typed frame
//!   parse at the boundary, permanent-vs-transient classification, and the
//!   level-triggered reconciler that replays never-dispatched forwards on every
//!   connect;
//! * [`Scheduler`] / [`FrameSink`] / [`OutboxHooks`] — the remaining host seams
//!   (timing+jitter, frame/status delivery, durable-outbox replay/settle).
//!
//! Nothing here pulls a timer, an HTTP client, or persistence: those are host IO
//! injected through the traits above, which is what keeps the crate compiling to
//! `wasm32-unknown-unknown` and on the frontier CI list. The browser binding +
//! wasm-bindgen boundary live in `posthaste-link-wasm`.

pub mod config;
pub mod engine;
pub mod error;
pub mod outbox;
pub mod scheduler;
pub mod sink;
pub mod transport;

pub use config::{BackoffPolicy, NearEndConfig};
pub use engine::{EngineError, NearEnd};
pub use error::{classify_status, Disposition};
pub use outbox::OutboxHooks;
pub use scheduler::Scheduler;
pub use sink::{ConnectionStatus, FrameSink};
pub use transport::{
    PostRequest, PostResponse, StreamEvent, StreamRequest, Transport, TransportError,
};
