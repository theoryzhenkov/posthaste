//! Permanent-vs-transient classification — one rule, extensible.
//!
//! The TS client split this fact three ways (`FatalStreamError` on 4xx in
//! `httpAdapter`, silent transient elsewhere, nothing on the forward path). The
//! engine owns it once: a 4xx *status* is a permanent verdict (the request is
//! malformed / unauthorized / gone — retrying cannot help); everything else
//! (5xx, network, timeout, clean close) is transient and reconnect-eligible.

/// How the engine treats a failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition {
    /// Retrying cannot help — stop and surface.
    Permanent,
    /// A retry (with backoff) may succeed — reconnect / re-forward.
    Transient,
}

impl Disposition {
    pub fn is_permanent(self) -> bool {
        matches!(self, Disposition::Permanent)
    }
}

/// Classify an HTTP status. 4xx (client error) is permanent; 2xx is success (a
/// non-failure — callers only pass failing statuses, but 2xx maps to transient
/// so an unexpected non-4xx is retried rather than fatally stopping). This is
/// the single extension point: add a status band here and both the forward path
/// and the stream loop inherit it.
pub fn classify_status(status: u16) -> Disposition {
    if (400..500).contains(&status) {
        Disposition::Permanent
    } else {
        Disposition::Transient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_hundreds_are_permanent() {
        for status in [400, 401, 403, 404, 409, 422, 499] {
            assert_eq!(classify_status(status), Disposition::Permanent, "{status}");
        }
    }

    #[test]
    fn five_hundreds_and_others_are_transient() {
        for status in [500, 502, 503, 504, 200, 204, 302] {
            assert_eq!(classify_status(status), Disposition::Transient, "{status}");
        }
    }
}
