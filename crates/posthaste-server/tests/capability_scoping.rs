//! Stage B integration tests: ATTENUATED macaroons (carrying first-party
//! caveats) actually restrict access. Built on real route templates so the auth
//! middleware resolves a `MatchedPath` and looks up the authz map exactly as in
//! production. A fixed test root key lets the test mint + attenuate real tokens.
//!
//! The 401-vs-403 split is load-bearing and asserted throughout: a forged token
//! is 401 (Unauthorized); an authentic token whose caveats are out of scope is
//! 403 (Forbidden).
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

#[path = "capability_scoping/basic_cases.rs"]
mod basic_cases;
#[path = "capability_scoping/filter_cases.rs"]
mod filter_cases;
#[path = "capability_scoping/mint_cases.rs"]
mod mint_cases;
#[path = "capability_scoping/support.rs"]
mod support;
