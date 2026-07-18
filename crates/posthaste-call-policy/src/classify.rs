//! Permanent-vs-transient classification (D82) over the *shared* taxonomy.
//!
//! O2 ruling: the retryability taxonomy is [`posthaste_domain_model::Terminality`]
//! — this crate **consumes** it, never forks it. Two pure functions live here:
//! the HTTP status-band fallback, and the envelope-over-status **precedence
//! rule**. The (M31) native provider executor calls these instead of
//! re-deriving the rule per site.

pub use posthaste_domain_model::Terminality;

/// Classify a bare HTTP status band into the shared [`Terminality`].
///
/// 4xx (client error) is **permanent** — the request is malformed / unauthorized
/// / gone, and retrying it as written cannot help. Everything else (5xx, and any
/// non-4xx a caller passes) is **transient** and retry-eligible, so an unexpected
/// status is retried rather than fatally stopping. This is the single band table:
/// add a band here and every consumer inherits it.
pub fn classify_status(status: u16) -> Terminality {
    if (400..500).contains(&status) {
        Terminality::Permanent
    } else {
        Terminality::Transient
    }
}

/// The envelope-over-status **precedence rule** (D82).
///
/// When a failing response carried a typed [`Terminality`] in its error envelope,
/// that verdict is **authoritative** — a far end that explicitly marked a 4xx
/// *transient* (or a 5xx *permanent*) is trusted. Only when no typed verdict is
/// present does the HTTP status band ([`classify_status`]) decide. This is
/// exactly the rule the near-end engine's `EngineError::from_response` open-coded;
/// centralizing it keeps the "envelope wins, band is fallback" semantics in one
/// testable place.
pub fn resolve_terminality(envelope: Option<Terminality>, status: u16) -> Terminality {
    envelope.unwrap_or_else(|| classify_status(status))
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

    #[test]
    fn envelope_terminality_wins_over_the_status_band() {
        // A 4xx the far end marked transient is transient (retry-eligible)...
        assert_eq!(
            resolve_terminality(Some(Terminality::Transient), 422),
            Terminality::Transient
        );
        // ...and a 5xx it marked permanent is permanent (stop).
        assert_eq!(
            resolve_terminality(Some(Terminality::Permanent), 503),
            Terminality::Permanent
        );
    }

    #[test]
    fn status_band_is_the_fallback_when_no_envelope() {
        assert_eq!(resolve_terminality(None, 404), Terminality::Permanent);
        assert_eq!(resolve_terminality(None, 500), Terminality::Transient);
    }
}
