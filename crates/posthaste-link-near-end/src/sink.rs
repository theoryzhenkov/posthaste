//! Where the engine delivers what it produces: typed frames + lifecycle status.
//!
//! The host (the browser `entityStoreAdapter`, via the wasm boundary) implements
//! this to receive frames the engine has already parsed and validated, plus
//! connection-state transitions it used to infer from raw stream callbacks. The
//! engine never hands the host an unparsed frame — malformed input is reported
//! separately and dropped, never cast.

use posthaste_contract_core::RuntimeFrame;

/// A connection-lifecycle transition the engine reports so the host can drive
/// degraded-mode UI (the flat-retry TS client had none — lifecycle-debt row 8).
#[derive(Clone, Debug)]
pub enum ConnectionStatus {
    /// A connect attempt is in flight (initial or reconnect).
    Connecting,
    /// The frame stream is open and live.
    Connected,
    /// The stream ended cleanly; the engine will reconnect after backoff.
    Reconnecting,
    /// A transient error; the engine will reconnect after backoff. Carries a
    /// human-readable reason for logging/UI.
    TransientError(String),
    /// A permanent error (4xx): the engine has stopped. The host must decide
    /// whether to re-establish (e.g. re-auth then a fresh connect).
    PermanentError(String),
}

/// The engine's output surface. Object-safe: held as `Rc<dyn FrameSink>`.
pub trait FrameSink {
    /// A parsed, validated runtime frame in arrival order.
    fn on_frame(&self, frame: RuntimeFrame);

    /// A stream payload that failed to parse into a [`RuntimeFrame`]. Dropped,
    /// not cast (lifecycle-debt row 9). `raw` is the offending payload; `error`
    /// the parse message.
    fn on_malformed(&self, raw: String, error: String);

    /// A connection-lifecycle transition.
    fn on_status(&self, status: ConnectionStatus);
}
