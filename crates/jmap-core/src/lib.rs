//! Core domain types and service logic for JMAP mail operations.
//!
//! This crate defines the domain model, port traits (gateway, store, secrets, config),
//! and the [`MailService`] orchestrator that composes them. No I/O or framework
//! dependencies live here; adapters are provided by sibling crates.
//!
//! @spec spec/L1-jmap
//! @spec spec/L0-api

mod config;
mod model;
mod ports;
mod push;
mod service;

pub use config::*;
pub use model::*;
pub use ports::*;
pub use push::*;
pub use service::*;
