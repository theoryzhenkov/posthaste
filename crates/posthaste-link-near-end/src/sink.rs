//! Where the engine delivers what it produces: typed frames + lifecycle status.
//!
//! The host (the browser `entityStoreAdapter`, via the wasm boundary) implements
//! this to receive frames the engine has already parsed and validated, plus
//! connection-state transitions it used to infer from raw stream callbacks. The
//! engine never hands the host an unparsed frame — malformed input is reported
//! separately and dropped, never cast.

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
    /// The wire is repeatedly delivering unparseable frames — a version skew or
    /// a corrupt peer ([3]). The stream keeps trying (this is not fatal), but the
    /// host should surface degraded-mode UI: the near node is no longer applying
    /// updates it cannot understand. Carries the offending count/reason.
    Degraded(String),
    /// A permanent error (4xx, or N consecutive malformed frames — [3]): the
    /// engine has stopped. The host must decide whether to re-establish (e.g.
    /// re-auth then a fresh connect).
    PermanentError(String),
}

/// The engine's output surface, generic over the seam's frame type
/// ([`crate::wire::Wire::Frame`]). Object-safe: held as `Rc<dyn FrameSink<F>>`.
pub trait FrameSink<Frame> {
    /// A parsed, validated frame in arrival order.
    fn on_frame(&self, frame: Frame);

    /// A stream payload that failed to parse into the seam's frame. Dropped,
    /// not cast (lifecycle-debt row 9). `raw` is the offending payload; `error`
    /// the parse message.
    fn on_malformed(&self, raw: String, error: String);

    /// The near node's incremental view of the stream is broken and must be
    /// rebuilt from scratch (D49): a detected seq gap the far-end could not
    /// replay (it sent a `Reset`), or an out-of-band resync. The host discards
    /// its stale incremental state and re-seeds — the native AS near-end evicts
    /// its whole read cache and re-reads through; the wasm client re-seeds the
    /// adapter (its existing view-refresh path). Default: no-op (seams that carry
    /// no reset are unaffected).
    fn on_reset(&self) {}

    /// A connection-lifecycle transition.
    fn on_status(&self, status: ConnectionStatus);

    /// The engine re-prepared a **fresh** link (a NEW link id) after the prior
    /// one went stale/absent (404/410 → re-prepare, D110a) — the recovery edge
    /// M44's reconcile keys off. `link_id` is the new connection token.
    ///
    /// Fired ONLY on a genuine re-prepare, never on a same-link reconnect (a
    /// 5xx / network blip keeps `prepared` and re-subscribes the SAME link, so
    /// no server-side view/cursor state was invalidated). The host adopts the
    /// new id and re-drives its server-served views + drifted caches against it;
    /// nothing here changes the frame stream, which the engine resumes itself.
    /// Default: no-op (seams whose host does not reconcile are unaffected).
    fn on_link_reestablished(&self, _link_id: String) {}
}
