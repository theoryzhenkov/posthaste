//! Integration tests for the loopback trust-model middleware.
//!
//! The headline invariant: with `require_auth` off (the default), an
//! un-tokened request still succeeds, so shipped behavior is byte-identical.
//! With it on, the bearer token + Origin/Host guard are enforced, with the
//! documented exemptions.
//!
//! @spec docs/eph/DESIGN-L1-trust-model

#[path = "auth_middleware/flag_cases.rs"]
mod flag_cases;
#[path = "auth_middleware/host_cases.rs"]
mod host_cases;
#[path = "auth_middleware/preflight_error_cases.rs"]
mod preflight_error_cases;
#[path = "auth_middleware/read_cases.rs"]
mod read_cases;
#[path = "auth_middleware/support.rs"]
mod support;
