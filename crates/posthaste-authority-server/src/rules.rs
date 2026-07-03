//! The in-process automation **rule engine** (RFC-L2-scripting S5, levels 0-1).
//!
//! Rules run *at the authority server*: the engine subscribes in-process to the
//! same domain-event broadcast the tap rides (RFC §8), and on a triggering fact
//! it loads the named message, evaluates the rule's WHEN-clause against it via
//! the shared query grammar, and — on a match — executes one action:
//!
//! * **Level 0** — `tag` / `move` / `notify`: built-ins that act through the
//!   authority server's own `apply` surface in-process (D53: one vocabulary).
//! * **Level 1** — `webhook` / `exec`: a POST to a URL, or a local script, each
//!   handed a **per-invocation, attenuated capability token** minted to exactly
//!   the rule's grants + expiry, plus a deterministic idempotency key so an
//!   at-least-once redelivery cannot double-execute.
//!
//! # Exec trust model (READ THIS)
//!
//! [`RuleAction::Exec`](posthaste_domain_model::RuleAction::Exec) runs a LOCAL
//! command on the authority-server host. It is **config-file-only**: rules are
//! authored by editing `rules.toml` on the host, and the REST surface is
//! **read-only** (list + preview). This is a hard design rule, not an
//! oversight: a REST-settable exec action would be **remote code execution** —
//! anyone able to create a rule could run arbitrary commands on the server. The
//! same trust boundary is why a webhook `url` is trusted (config-authored) and
//! not treated as attacker-controlled SSRF surface. Do not add a create/edit
//! REST path for rules without revisiting this.

use std::sync::Arc;

mod actions;
mod config;
mod engine;
mod writer;

pub use config::{load_rules, RuleConfigError};
pub use engine::{ManagedRulesHandle, RuleEngineHandle};
pub use writer::RuleWriteError;

pub(crate) use engine::spawn as spawn_engine;

/// Structured inputs for minting one per-invocation capability token (D53). The
/// engine builds this from a rule's grants + expiry, scoped to the matched
/// account + message; the host-supplied [`CapabilityMinter`] turns it into a
/// signed, attenuated token.
#[derive(Clone, Debug, Default)]
pub struct RuleTokenGrant {
    /// The authz verbs the token carries (`read`, `send`, `tag`, `move`,
    /// `delete`). Rendered into a single `action = a,b,c` caveat.
    pub actions: Vec<String>,
    /// The account the token is confined to (`account = …` caveat).
    pub account: Option<String>,
    /// The single message the token is confined to (`message = …` caveat) —
    /// least privilege: the hook can only touch the message that triggered it.
    pub message: Option<String>,
    /// The token expiry as an RFC3339 timestamp (`expires = …` caveat).
    pub expiry_rfc3339: Option<String>,
}

/// The capability-minting **port** (dependency inversion). The rule engine lives
/// in the authority server, which must not depend on the HTTP adapter's macaroon
/// machinery; the bundled host (which owns the macaroon root key) supplies the
/// concrete minter. When absent, Level-1 hook actions cannot run and dead-letter
/// with a "no capability minter" reason.
pub trait CapabilityMinter: Send + Sync {
    /// Mint a fresh capability token carrying exactly the caveats implied by
    /// `grant`. Returns the token string, or an error description.
    fn mint(&self, grant: &RuleTokenGrant) -> Result<String, String>;
}

/// A boxed, shareable [`CapabilityMinter`].
pub type SharedMinter = Arc<dyn CapabilityMinter>;
