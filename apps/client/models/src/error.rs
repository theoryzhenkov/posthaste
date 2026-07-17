//! The one failure envelope for the whole API: a typed kind, a
//! human-readable message, and a retryability flag. Clients match on the
//! kind, never on the message text.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The error body every failed HTTP call carries. Provider failures do not
/// appear here — an accepted command's later failure is queryable verdict
/// state, not an HTTP error.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub kind: ApiErrorKind,
    /// Human-readable description, for logs and fallback display.
    pub message: String,
    /// Whether retrying the same request unchanged can succeed (e.g. after a
    /// transient backend condition clears).
    pub retryable: bool,
}

/// The typed failure kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ApiErrorKind {
    /// The request body did not parse or validate as a known shape.
    MalformedRequest,
    /// No or invalid credentials (session secret / token).
    Unauthorized,
    /// The credential is valid but its capability set does not cover this
    /// query, command, or account.
    CapabilityDenied,
    /// A referenced id (account, message, mailbox, blob, operation) does not
    /// exist.
    UnknownId,
    /// The request conflicts with current state (e.g. a duplicate create).
    Conflict,
    /// The backend cannot serve the request right now; retry later.
    Unavailable,
    /// An unexpected backend failure.
    Internal,
}
