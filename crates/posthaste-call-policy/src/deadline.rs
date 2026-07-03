//! The per-class deadline table (D81) — the F2 fix, encoded as data in one place.
//!
//! F2's defect was a single 10 s **total** timeout governing a keyword flip and
//! a 20 MB blob alike: a total timeout applied to a *streaming* body fails a
//! slow-but-alive download deterministically forever. The fix is four call
//! classes, each with the deadline **shape** it actually needs — a total, a
//! stall (idle-read) deadline, or neither — named once here so the executor and
//! the link engine tune from the same table.

use std::time::Duration;

/// What *kind* of outbound call this is — the axis the deadline shape keys off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallClass {
    /// A short metadata read / mutation (keyword flip, `*/set`, folder list).
    /// Governed by a total wall-clock deadline.
    Metadata,
    /// A blob download/upload (attachment, raw message). Governed by a *stall*
    /// (no-bytes-for-N) read-deadline and **no** total — a large body on a slow
    /// link must be allowed to finish (F2).
    Blob,
    /// An outbound message submission. Governed by its own total deadline whose
    /// expiry classifies as *dispatch-uncertain* (D86), never a blind-retryable
    /// transient — the total is coupled to the send semantics, not F2's monoculture.
    Send,
    /// A push/frame subscription. Long-lived: the deadline governs the *connect*
    /// handshake only; ongoing liveness is a keepalive/read-deadline concern
    /// (D88), not a total on the stream.
    Subscribe,
}

/// The deadline shape for a call: a total wall-clock ceiling and/or an
/// idle-read (stall) ceiling. Both optional — a class may want one, the other,
/// or (for a long-lived subscription past connect) neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlinePolicy {
    /// Wall-clock ceiling on the whole call. `None` = no total (blob/subscribe).
    /// For [`CallClass::Subscribe`] the value governs the connect handshake only.
    pub total: Option<Duration>,
    /// Idle-read ceiling: fail if no bytes arrive for this long. `None` = not a
    /// streamed body.
    pub stall: Option<Duration>,
}

// ---- the ratified defaults, named once (all **Review**/tunable) ------------

/// [`CallClass::Metadata`] total deadline (D81: "~30 s, tunable").
pub const METADATA_TOTAL: Duration = Duration::from_secs(30);
/// [`CallClass::Blob`] stall (idle-read) deadline — no-bytes-for-N.
pub const BLOB_STALL: Duration = Duration::from_secs(30);
/// [`CallClass::Send`] total deadline; its expiry is *dispatch-uncertain* (D86).
pub const SEND_TOTAL: Duration = Duration::from_secs(60);
/// [`CallClass::Subscribe`] connect-handshake deadline.
pub const SUBSCRIBE_CONNECT: Duration = Duration::from_secs(30);

impl CallClass {
    /// The ratified [`DeadlinePolicy`] for this class (D81, the F2 fix). This is
    /// the single tuning surface: change a constant above and both the link
    /// engine and the native executor inherit it.
    pub fn deadline_policy(self) -> DeadlinePolicy {
        match self {
            CallClass::Metadata => DeadlinePolicy {
                total: Some(METADATA_TOTAL),
                stall: None,
            },
            CallClass::Blob => DeadlinePolicy {
                total: None,
                stall: Some(BLOB_STALL),
            },
            CallClass::Send => DeadlinePolicy {
                total: Some(SEND_TOTAL),
                stall: None,
            },
            CallClass::Subscribe => DeadlinePolicy {
                total: Some(SUBSCRIBE_CONNECT),
                stall: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_gets_a_total_no_stall() {
        let p = CallClass::Metadata.deadline_policy();
        assert_eq!(p.total, Some(METADATA_TOTAL));
        assert_eq!(p.stall, None);
    }

    #[test]
    fn blob_gets_a_stall_and_never_a_total() {
        let p = CallClass::Blob.deadline_policy();
        assert_eq!(p.total, None, "F2: a total on a streamed body is the bug");
        assert_eq!(p.stall, Some(BLOB_STALL));
    }

    #[test]
    fn send_gets_its_own_total() {
        let p = CallClass::Send.deadline_policy();
        assert_eq!(p.total, Some(SEND_TOTAL));
        assert_eq!(p.stall, None);
    }

    #[test]
    fn subscribe_gets_a_connect_only_deadline() {
        let p = CallClass::Subscribe.deadline_policy();
        assert_eq!(p.total, Some(SUBSCRIBE_CONNECT));
        assert_eq!(p.stall, None);
    }
}
