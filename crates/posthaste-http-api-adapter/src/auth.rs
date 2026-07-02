//! Loopback bearer-token + Origin/Host guard for the `/v1` API.
//!
//! Gated behind `[daemon] require_auth` (default `true`). When the flag is
//! off (explicit opt-out) the middleware is a pass-through, so the no-auth
//! behavior is byte-identical. When on:
//!
//! - The `Host` header is validated against an allowlist on **every** request
//!   (including the otherwise-exempt liveness/doc routes), independent of
//!   `Origin`. This is the load-bearing DNS-rebinding defense: a rebinding
//!   attack reaches us as `Host: attacker.com` with no `Origin`, which the
//!   Origin check alone would wave through.
//! - A matching `Authorization: Bearer <token>` (the per-process token) is
//!   required, except for a small set of exempt liveness/doc routes.
//! - Browser requests carrying an `Origin`/`Referer` are additionally checked
//!   against an origin allowlist (CSRF defense-in-depth).
//!
//! @spec docs/eph/DESIGN-L1-trust-model

mod context;
mod errors;
mod middleware;
mod perimeter;

pub use middleware::require_auth_layer;
pub use perimeter::{host_allowlist, origin_allowlist};
// Shared with the runtime↔authority-server link auth (`link.rs` in `posthaste-server`):
// one bearer-parse + one constant-time compare + the canonical 401, so the link
// surface enforces the same way the `/v1` perimeter does.
pub use errors::unauthorized;
pub use perimeter::{bearer_token, constant_time_eq};

#[cfg(test)]
use perimeter::{bind_host, is_exempt_path, normalize_host_header, origin_allowed};

/// The verified bearer token, placed in request extensions by
/// [`require_auth_layer`] once authenticity is confirmed. Handlers that mint a
/// derived token (the `POST /v1/auth/tokens` endpoint) read it and **attenuate**
/// it, which can only add caveats — so a minted token never exceeds the
/// caller's authority. Absent when `require_auth` is off (no caller token).
#[derive(Clone)]
pub struct PresentedToken(pub String);

#[cfg(test)]
mod tests;
