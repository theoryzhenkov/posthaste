//! Protocol models for the client API: queries, query answers, commands,
//! the event stream, and the error envelope. The single source of truth for
//! both ends — TypeScript types are GENERATED from this crate (ts-rs) into
//! `frontend/src/gen/` via the `export-ts` binary (`just gen-ts`); nothing
//! protocol-shaped is hand-written twice.
//!
//! Wire shapes reuse the domain-model types directly (message summaries,
//! mailbox summaries, recipients, ids, mail commands) so the backend
//! serializes domain values without conversion. The domain crate does not
//! derive `ts_rs::TS`, so [`mirror`] declares TypeScript-shape twins for the
//! reused types; wire fields point at them with `#[ts(as = ...)]`, and a
//! drift test keeps mirror and domain serde-identical.
//!
//! Dependency allowlist: `serde` + `ts-rs` + the domain model, nothing else.

pub mod codegen;
pub mod command;
pub mod error;
pub mod event;
pub mod mirror;
pub mod query;

pub use command::*;
pub use error::*;
pub use event::*;
pub use query::*;
