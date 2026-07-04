use super::*;

/// The one typed retryability axis (RFC-L2 D70, tenet XIV "one shared fact, one
/// type"): does retrying an operation stand any chance of a different outcome?
///
/// This is deliberately a two-value vocabulary — it answers *retryability* and
/// nothing else:
///
/// * **Availability** — D49's `Degraded` — is an *orthogonal* state that
///   composes with a `Terminality`, not a third variant here (RFC ruling 4). A
///   degraded provider still produces `Transient` failures; degradation is
///   tracked on the account/connection status, not folded into this enum.
/// * **The reason** a failure earned its terminality (auth vs corruption vs an
///   internal decode bug) is carried by the *paired* typed code at each site —
///   the [`RuntimeErrorCode`](../../posthaste_contract_core) on a
///   `RuntimeAdapterError`, or the [`GatewayError`] variant behind an outbox
///   flush. `Terminality` is the shared verdict; the code alongside it is the
///   shared reason. This keeps the axis small enough to be the single thing
///   three consumers (outbox flush, near-end engine, D47 settlement) agree on.
///
/// Serde-only, no I/O deps: it rides the wasm frontier embedded in
/// `RuntimeAdapterError`.
///
/// @spec docs/eph/RFC-L2-lifecycle-and-errors#3b-errors
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Terminality {
    /// A retry — after backoff, reconnect, or re-auth — may succeed.
    Transient,
    /// Retrying the operation as written cannot succeed; it must change or fail.
    Permanent,
}

impl Terminality {
    /// Whether a retry may succeed (the reconnect/re-drain-eligible case).
    pub fn is_transient(self) -> bool {
        matches!(self, Self::Transient)
    }

    /// Whether retrying is futile (surface-and-stop).
    pub fn is_permanent(self) -> bool {
        matches!(self, Self::Permanent)
    }
}

/// Errors from JMAP gateway operations.
///
/// @spec docs/L1-jmap#error-model
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway unavailable for account {0}")]
    Unavailable(String),
    #[error("authentication failed")]
    Auth,
    #[error("network error: {0}")]
    Network(String),
    /// A **send-class** call whose delivery outcome is unknown: the request
    /// timed out or the transport was lost *after* the submission may already
    /// have committed server-side. Distinct from [`Self::Network`] (a clear
    /// pre-commit transport error, safe to retry) because a possibly-delivered
    /// send must **never** be blind-resent — the outbox parks it in
    /// `DispatchUncertain` (RFC-L2 D86) rather than looping it as `Transient`.
    ///
    /// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
    #[error("send dispatch uncertain: {0}")]
    DispatchUncertain(String),
    #[error("state mismatch")]
    StateMismatch,
    #[error("cannot calculate changes")]
    CannotCalculateChanges,
    #[error("gateway rejected the request: {0}")]
    Rejected(String),
    /// The local store hit corruption while serving a gateway op (e.g. an IMAP
    /// mutation reading local UID state). Kept distinct from [`Self::Rejected`]
    /// so the corrupt-store repair pathway (`storage_corrupted`, which the web
    /// client handles specifically) survives the hop instead of masquerading as
    /// a provider-side rejection (audit top-10 #4).
    #[error("gateway hit a corrupt local store: {0}")]
    Corruption(String),
    /// An internal serialization/decode bug on our side of the exchange — not a
    /// provider fault. Retrying the network cannot fix a payload our own code
    /// cannot encode or decode, so it is classified permanent rather than
    /// mislabeled `Network`/`Rejected` (audit §2 serde-decode edges).
    #[error("internal gateway codec error: {0}")]
    Internal(String),
    /// The provider rejected a message mutation, but a `set`+`get` still read the
    /// message's current (unchanged) state back. The readback drives optimistic
    /// settlement — writing it reverts the rejected change — while the typed
    /// error lets the flush surface the failure to the user.
    ///
    /// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    #[error("gateway rejected the mutation: {reason}")]
    MutationRejected {
        readback: Box<MessageReadback>,
        reason: String,
    },
}

/// Errors from the local SQLite store.
///
/// @spec docs/L1-sync#error-handling
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage failure: {0}")]
    Failure(String),
    /// The SQLite database file is corrupt (e.g. "database disk image is
    /// malformed"). Distinguished from a generic failure so callers can offer a
    /// repair pathway: the store is a rebuildable projection, so the corrupt
    /// file can be quarantined and recreated.
    #[error("database corrupted: {0}")]
    Corruption(String),
}

/// Unified error type surfaced by [`crate::MailService`] and mapped to HTTP status codes.
///
/// @spec docs/L1-api#error-format
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Gateway(#[from] GatewayError),
    #[error(transparent)]
    Secret(#[from] SecretStoreError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
}

/// Stable service error category for exhaustive API status mapping.
///
/// @spec docs/L1-api#error-code-mapping
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceErrorKind {
    GatewayUnavailable,
    AuthError,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    NotFound,
    Conflict,
    StorageFailure,
    StorageCorrupted,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
    /// An internal codec/logic fault surfaced through a gateway op (a serialize
    /// or decode bug on our side). Distinct from `GatewayRejected` so it maps to
    /// a 500 rather than a client-facing 400.
    Internal,
}

impl ServiceErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::GatewayUnavailable => "gateway_unavailable",
            Self::AuthError => "auth_error",
            Self::NetworkError => "network_error",
            Self::StateMismatch => "state_mismatch",
            Self::CannotCalculateChanges => "cannot_calculate_changes",
            Self::GatewayRejected => "gateway_rejected",
            Self::SecretUnavailable => "secret_unavailable",
            Self::SecretUnsupported => "secret_unsupported",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::StorageFailure => "storage_failure",
            Self::StorageCorrupted => "storage_corrupted",
            Self::ConfigValidation => "config_validation",
            Self::ConfigIo => "config_io",
            Self::ConfigParse => "config_parse",
        }
    }
}

impl ServiceError {
    /// Returns the stable category used for API status mapping.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn kind(&self) -> ServiceErrorKind {
        match self {
            Self::Gateway(GatewayError::Unavailable(_)) => ServiceErrorKind::GatewayUnavailable,
            Self::Gateway(GatewayError::Auth) => ServiceErrorKind::AuthError,
            Self::Gateway(GatewayError::Network(_)) => ServiceErrorKind::NetworkError,
            // A dispatch-uncertain send is intercepted at the outbox flush layer
            // (parked, never resent) and does not surface as a `ServiceError` to
            // an API caller; it shares the network-class code should it ever be
            // mapped, since it is a transport-condition on a possibly-live call.
            Self::Gateway(GatewayError::DispatchUncertain(_)) => ServiceErrorKind::NetworkError,
            Self::Gateway(GatewayError::StateMismatch) => ServiceErrorKind::StateMismatch,
            Self::Gateway(GatewayError::CannotCalculateChanges) => {
                ServiceErrorKind::CannotCalculateChanges
            }
            Self::Gateway(GatewayError::Rejected(_))
            | Self::Gateway(GatewayError::MutationRejected { .. }) => {
                ServiceErrorKind::GatewayRejected
            }
            // A corrupt local store surfaced through a gateway op keeps the
            // storage-corruption class end-to-end (→ `storage_corrupted`).
            Self::Gateway(GatewayError::Corruption(_)) => ServiceErrorKind::StorageCorrupted,
            // An internal codec bug is a 500-class internal fault, not a
            // provider rejection.
            Self::Gateway(GatewayError::Internal(_)) => ServiceErrorKind::Internal,
            Self::Secret(SecretStoreError::Unavailable(_)) => ServiceErrorKind::SecretUnavailable,
            Self::Secret(SecretStoreError::Unsupported(_)) => ServiceErrorKind::SecretUnsupported,
            Self::Store(StoreError::NotFound(_)) | Self::Config(ConfigError::NotFound(_)) => {
                ServiceErrorKind::NotFound
            }
            Self::Store(StoreError::Conflict(_)) | Self::Config(ConfigError::Conflict(_)) => {
                ServiceErrorKind::Conflict
            }
            Self::Store(StoreError::Failure(_)) => ServiceErrorKind::StorageFailure,
            Self::Store(StoreError::Corruption(_)) => ServiceErrorKind::StorageCorrupted,
            Self::Config(ConfigError::Validation(_)) => ServiceErrorKind::ConfigValidation,
            Self::Config(ConfigError::Io(_)) => ServiceErrorKind::ConfigIo,
            Self::Config(ConfigError::Parse(_)) => ServiceErrorKind::ConfigParse,
        }
    }

    /// Returns the error code string used in the JSON error response body.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn code(&self) -> &'static str {
        self.kind().code()
    }
}

/// The small, stable set of **user-facing** account-error categories the UI
/// groups sync/connection failures into (RFC-L2-client-resilience M45).
///
/// This is a third, orthogonal axis distinct from the two typed axes it sits
/// beside:
///
/// * [`Terminality`] answers *retryability* (does a retry stand a chance).
/// * [`ServiceErrorKind`] is the *wire code* for HTTP status mapping.
/// * `AccountErrorCategory` is the *presentation* axis: what a human should be
///   told and which recovery affordance to offer.
///
/// It exists so the account status surface (and any toast) can render a clear
/// "what happened + what to do" without ever leaking a raw provider/library
/// string (audit top-10 #7). The paired [`user_facing`](ServiceError::user_facing)
/// classifier is the single mapping from a typed error to this axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountErrorCategory {
    /// A connection/transport failure: TCP connect refused, DNS, TLS, timeout,
    /// host unreachable, provider offline. Auto-retried by the sync poll loop.
    Network,
    /// Sign-in failed or the grant/token expired — the user must reconnect.
    Auth,
    /// The provider is throttling us (HTTP 429 / "rate limit" / "too many
    /// requests"). Auto-retried after backoff.
    RateLimited,
    /// The account's server settings or a request are wrong — the user should
    /// review the account configuration.
    Config,
    /// The local mail database is damaged or unreadable — a repair may help.
    Storage,
    /// An unexpected internal fault on our side; retried automatically.
    Internal,
}

impl AccountErrorCategory {
    /// Whether the supervisor auto-retries this class (so the UI can say
    /// "retrying automatically" rather than demanding user action).
    pub fn is_auto_retrying(self) -> bool {
        matches!(self, Self::Network | Self::RateLimited | Self::Internal)
    }
}

/// A user-facing rendering of an account error: a coarse [category](AccountErrorCategory),
/// a human message that NEVER contains a raw library/provider string, and the
/// stable code carried on the account runtime overview for the client
/// presentation layer to re-classify against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserFacingError {
    pub category: AccountErrorCategory,
    pub message: String,
    pub code: &'static str,
}

/// Case-insensitive substring scan for provider throttling signatures. The IMAP
/// classifier flattens a rate-limit into `GatewayError::Network(message)`
/// carrying the provider's own text, so the only signal left is the string —
/// inspected here (and nowhere else) to split `RateLimited` out of `Network`.
fn looks_rate_limited(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("rate limit")
        || detail.contains("ratelimit")
        || detail.contains("throttl")
        || detail.contains("too many requests")
        || detail.contains("429")
}

impl ServiceError {
    /// Classify this error into the [`UserFacingError`] presentation axis
    /// (M45). This is the single server-side mapping from a typed error to a
    /// human-readable category + message; it is applied where errors are
    /// recorded onto account status so the raw error string never reaches the
    /// UI. The client presentation layer re-classifies against the returned
    /// `code` to add provider-aware phrasing and the recovery action.
    pub fn user_facing(&self) -> UserFacingError {
        let kind = self.kind();
        match kind {
            ServiceErrorKind::NetworkError | ServiceErrorKind::GatewayUnavailable => {
                if looks_rate_limited(&self.to_string()) {
                    UserFacingError {
                        category: AccountErrorCategory::RateLimited,
                        message: "The mail provider is throttling requests — retrying shortly."
                            .to_string(),
                        code: "rate_limited",
                    }
                } else {
                    UserFacingError {
                        category: AccountErrorCategory::Network,
                        message:
                            "Couldn't reach the mail server — check your connection. Retrying automatically."
                                .to_string(),
                        code: kind.code(),
                    }
                }
            }
            ServiceErrorKind::AuthError => UserFacingError {
                category: AccountErrorCategory::Auth,
                message: "Sign-in expired — reconnect your account.".to_string(),
                code: kind.code(),
            },
            ServiceErrorKind::SecretUnavailable | ServiceErrorKind::SecretUnsupported => {
                UserFacingError {
                    category: AccountErrorCategory::Auth,
                    message: "This account's saved credentials are unavailable — reconnect your account."
                        .to_string(),
                    code: kind.code(),
                }
            }
            ServiceErrorKind::StorageCorrupted => UserFacingError {
                category: AccountErrorCategory::Storage,
                message: "The local mail database is damaged — a repair may be needed.".to_string(),
                code: kind.code(),
            },
            ServiceErrorKind::StorageFailure => UserFacingError {
                category: AccountErrorCategory::Storage,
                message: "Couldn't read the local mail database — retrying.".to_string(),
                code: kind.code(),
            },
            ServiceErrorKind::GatewayRejected
            | ServiceErrorKind::StateMismatch
            | ServiceErrorKind::CannotCalculateChanges
            | ServiceErrorKind::ConfigValidation
            | ServiceErrorKind::ConfigParse => UserFacingError {
                category: AccountErrorCategory::Config,
                message: "The mail server settings look wrong — check this account's configuration."
                    .to_string(),
                code: kind.code(),
            },
            ServiceErrorKind::NotFound
            | ServiceErrorKind::Conflict
            | ServiceErrorKind::ConfigIo
            | ServiceErrorKind::Internal => UserFacingError {
                category: AccountErrorCategory::Internal,
                message: "Something went wrong syncing this account — retrying.".to_string(),
                code: kind.code(),
            },
        }
    }
}

/// Errors from credential storage operations.
///
/// @spec docs/L1-api#secret-management
#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret unavailable: {0}")]
    Unavailable(String),
    #[error("secret store does not support operation: {0}")]
    Unsupported(String),
}

#[cfg(test)]
mod user_facing_tests {
    use super::*;

    #[test]
    fn tcp_connect_failure_is_network_not_raw() {
        // The real beta-critical symptom: a raw IMAP/TLS library string must
        // never pass through. It becomes a Network category with our message.
        let error = ServiceError::Gateway(GatewayError::Network(
            "cannot connect to TCP stream".to_string(),
        ));
        let presented = error.user_facing();
        assert_eq!(presented.category, AccountErrorCategory::Network);
        assert_eq!(presented.code, "network_error");
        assert!(presented.category.is_auto_retrying());
        // No raw library text leaks into the human message.
        assert!(!presented.message.contains("TCP stream"));
        assert!(presented.message.contains("check your connection"));
    }

    #[test]
    fn gateway_unavailable_is_network() {
        let error = ServiceError::Gateway(GatewayError::Unavailable("acct-1".to_string()));
        assert_eq!(
            error.user_facing().category,
            AccountErrorCategory::Network
        );
    }

    #[test]
    fn auth_failure_is_auth_with_reconnect_message() {
        let presented = ServiceError::Gateway(GatewayError::Auth).user_facing();
        assert_eq!(presented.category, AccountErrorCategory::Auth);
        assert_eq!(presented.code, "auth_error");
        assert!(!presented.category.is_auto_retrying());
        assert!(presented.message.to_lowercase().contains("reconnect"));
    }

    #[test]
    fn rate_limit_signature_splits_out_of_network() {
        for detail in [
            "429 Too Many Requests",
            "provider rate limit exceeded",
            "request throttled, retry later",
        ] {
            let presented =
                ServiceError::Gateway(GatewayError::Network(detail.to_string())).user_facing();
            assert_eq!(
                presented.category,
                AccountErrorCategory::RateLimited,
                "expected rate-limited for detail {detail:?}"
            );
            assert_eq!(presented.code, "rate_limited");
            assert!(presented.category.is_auto_retrying());
            assert!(!presented.message.contains(detail));
        }
    }

    #[test]
    fn rejected_is_config() {
        let presented =
            ServiceError::Gateway(GatewayError::Rejected("bad request".to_string())).user_facing();
        assert_eq!(presented.category, AccountErrorCategory::Config);
        assert!(!presented.message.contains("bad request"));
    }

    #[test]
    fn corruption_is_storage() {
        let presented =
            ServiceError::Store(StoreError::Corruption("disk image malformed".to_string()))
                .user_facing();
        assert_eq!(presented.category, AccountErrorCategory::Storage);
        assert_eq!(presented.code, "storage_corrupted");
        assert!(!presented.message.contains("malformed"));
    }

    #[test]
    fn secret_unavailable_is_auth() {
        let presented =
            ServiceError::Secret(SecretStoreError::Unavailable("keychain locked".to_string()))
                .user_facing();
        assert_eq!(presented.category, AccountErrorCategory::Auth);
        assert!(!presented.message.contains("keychain"));
    }
}
