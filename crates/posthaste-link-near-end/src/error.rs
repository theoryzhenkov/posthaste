//! Permanent-vs-transient classification — one rule, over the *shared*
//! vocabulary.
//!
//! The TS client split this fact three ways (`FatalStreamError` on 4xx in
//! `httpAdapter`, silent transient elsewhere, nothing on the forward path). The
//! engine owns it once, and it owns it in the workspace-wide type: the verdict
//! is [`Terminality`] (RFC-L2 D70), the same enum the outbox flush and the D47
//! settlement seam speak. A 4xx *status* is a permanent verdict (the request is
//! malformed / unauthorized / gone — retrying cannot help); everything else
//! (5xx, network, timeout, clean close) is transient and reconnect-eligible.
//!
//! The HTTP status band is only the *fallback* input: when a response carries a
//! typed [`Terminality`] in its envelope, the engine respects that instead
//! (see `EngineError::from_response`).

pub use posthaste_contract_core::Terminality;

/// Classify an HTTP status into the shared [`Terminality`]. 4xx (client error)
/// is permanent; 2xx is success (a non-failure — callers only pass failing
/// statuses, but 2xx maps to transient so an unexpected non-4xx is retried
/// rather than fatally stopping). This is the single extension point: add a
/// status band here and both the forward path and the stream loop inherit it.
pub fn classify_status(status: u16) -> Terminality {
    if (400..500).contains(&status) {
        Terminality::Permanent
    } else {
        Terminality::Transient
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_hundreds_are_permanent() {
        for status in [400, 401, 403, 404, 409, 422, 499] {
            assert_eq!(classify_status(status), Terminality::Permanent, "{status}");
        }
    }

    #[test]
    fn five_hundreds_and_others_are_transient() {
        for status in [500, 502, 503, 504, 200, 204, 302] {
            assert_eq!(classify_status(status), Terminality::Transient, "{status}");
        }
    }
}
