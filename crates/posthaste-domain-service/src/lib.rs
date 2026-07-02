//! Core domain service logic for JMAP mail operations — the hexagonal core.
//!
//! This crate holds the [`MailService`] orchestrator, the port traits (gateway,
//! store, secrets, config, push), provider policies, imap planning logic, cache
//! scoring/governor, and search parsing. Pure domain types live in the leaf
//! crate [`posthaste_domain_model`]; consumers that need those types import
//! them from there directly.
//!
//! @spec docs/L1-jmap
//! @spec docs/L0-api

pub mod cache;
mod config;
mod imap;
mod ports;
mod push;
pub mod search;
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
