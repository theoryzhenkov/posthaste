//! Core domain types and service logic for JMAP mail operations.
//!
//! This crate defines the domain model, port traits (gateway, store, secrets, config),
//! and the [`MailService`] orchestrator that composes them. No I/O or framework
//! dependencies live here; adapters are provided by sibling crates.
//!
//! @spec docs/L1-jmap
//! @spec docs/L0-api

pub mod cache;
mod config;
mod generated_id;
mod imap;
mod model;
mod ports;
mod provider;
mod push;
pub mod search;
mod secret;
mod service;
mod validation;
mod vocab;

pub use cache::*;
pub use config::*;
pub use generated_id::*;
pub use imap::*;
pub use model::*;
pub use ports::*;
pub use provider::*;
pub use push::*;
pub use secret::*;
pub use service::*;
pub use validation::*;
pub use vocab::*;
