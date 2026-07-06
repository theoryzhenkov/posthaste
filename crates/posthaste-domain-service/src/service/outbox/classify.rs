//! Gateway-error classification: map a provider failure to its typed
//! retryability verdict ([`FlushDisposition`]).

use crate::service::*;

/// How a push failure routes the operation. A superset of the D70 retryability
/// verdict ([`Terminality`]): the send path adds a third, non-blind-retryable
/// disposition (`Uncertain`) for a possibly-delivered send.
pub(super) enum FlushDisposition {
    /// The network recovers on its own — keep the op pending, stop draining
    /// (offline), retry next window.
    Transient,
    /// Retrying the same push cannot change the outcome — fail and surface.
    Permanent,
    /// A **send** whose delivery outcome is unknown (timeout/transport-loss
    /// after the submission may have committed). Park in `DispatchUncertain`
    /// (RFC-L2 D86) — never blind-resent; only an explicit user retry (under the
    /// same idempotency identity) or a discard resolves it.
    Uncertain,
}

/// Outcome of attempting to push one operation to the provider: the routing
/// [`FlushDisposition`] plus the human-readable message recorded on the
/// operation / surfaced in the settlement. The verdict is data, not a string
/// bucket.
pub(super) struct FlushError {
    pub(super) disposition: FlushDisposition,
    pub(super) message: String,
}

impl FlushError {
    /// A local, provider-independent permanent failure (e.g. an un-decodable
    /// stored payload) — there is no `GatewayError` to classify.
    pub(super) fn permanent(message: impl Into<String>) -> Self {
        Self {
            disposition: FlushDisposition::Permanent,
            message: message.into(),
        }
    }
}

/// Classify a gateway failure into its typed retryability verdict plus the
/// message recorded on the operation. Exhaustive over [`GatewayError`] by design
/// (the M29 gate): a new variant fails to compile here until its terminality is
/// decided — no `other => Permanent(to_string())` free-text catch-all.
pub(super) fn classify_gateway_error(error: GatewayError) -> FlushError {
    let (disposition, message) = match error {
        // Reachable-again: the network recovers on its own.
        GatewayError::Network(message) | GatewayError::Unavailable(message) => {
            (FlushDisposition::Transient, message)
        }
        // Auth failures are transient: they clear once the account re-authenticates.
        GatewayError::Auth => (
            FlushDisposition::Transient,
            "authentication required".to_string(),
        ),
        // A send whose outcome is unknown — the request may have committed after
        // the transport dropped. Never blind-resend: park it (D86).
        GatewayError::DispatchUncertain(message) => (FlushDisposition::Uncertain, message),
        // Terminal as written — a diverged state, a provider rejection, a corrupt
        // local store, or an internal codec bug — retrying the same push cannot
        // change the outcome.
        GatewayError::StateMismatch => (
            FlushDisposition::Permanent,
            "provider state diverged".to_string(),
        ),
        GatewayError::CannotCalculateChanges => (
            FlushDisposition::Permanent,
            "cannot calculate changes".to_string(),
        ),
        GatewayError::Rejected(message)
        | GatewayError::Corruption(message)
        | GatewayError::Internal(message) => (FlushDisposition::Permanent, message),
        GatewayError::MutationRejected { reason, .. } => (FlushDisposition::Permanent, reason),
        // Mailbox destroy is a synchronous mutation (never queued in the outbox),
        // so this refusal cannot actually reach the flush path; classify it
        // permanent for exhaustiveness — retrying the same push can't change it.
        GatewayError::MailboxNotEmpty { count } => (
            FlushDisposition::Permanent,
            format!("mailbox is not empty ({count} messages)"),
        ),
    };
    FlushError {
        disposition,
        message,
    }
}
