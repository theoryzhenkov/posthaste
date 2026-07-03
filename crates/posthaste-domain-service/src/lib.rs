//! Core domain service logic for JMAP mail operations — the hexagonal core.
//!
//! This crate holds the [`MailService`] orchestrator, the port traits (gateway,
//! store, secrets, config, push), provider policies, imap planning logic, and
//! cache scoring/governor. Pure domain types live in the leaf crate
//! [`posthaste_domain_model`]; consumers that need those types import them
//! from there directly. Query parsing (the smart-mailbox/search grammar) lives
//! in [`posthaste_query_grammar`] — extracted out per RFC-L2-scripting §7
//! ruling 4 so the rules engine can consume the same parser without depending
//! on this crate; consumers of the grammar import it directly rather than
//! through a re-export here (no facade, D19/XX).
//!
//! @spec docs/L1-jmap
//! @spec docs/L0-api

pub mod cache;
mod config;
mod imap;
mod ports;
mod push;
mod secret;
mod service;
mod validation;

pub use cache::*;
pub use config::*;
pub use imap::*;
pub use ports::*;
pub use push::*;
pub use secret::*;
pub use service::*;
pub use validation::*;
