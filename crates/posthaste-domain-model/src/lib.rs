//! Pure domain model types for Posthaste: ids, messages, records, commands,
//! outbox/sync/rev-log types, smart mailboxes, account settings/overview,
//! appearance, automation, notifications, vocab, errors, and the pure
//! cache/imap/provider type slices the model types embed.
//!
//! This is the lean leaf crate (`posthaste-domain-model`): serde-only, no I/O,
//! no framework dependencies. The hexagonal service core lives in
//! `posthaste-domain-service`.
//!
//! @spec docs/L1-jmap
//! @spec docs/L0-api

mod cache;
mod config;
mod generated_id;
mod imap;
mod model;
mod provider;
mod validation;
mod vocab;

pub use cache::*;
pub use config::*;
pub use generated_id::*;
pub use imap::*;
pub use model::*;
pub use provider::*;
pub use validation::*;
pub use vocab::*;
