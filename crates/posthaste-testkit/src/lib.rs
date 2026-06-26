//! Shared test support for Posthaste.
//!
//! Dev-only library consumed by integration tests via `[dev-dependencies]`.
//! Provides the disposable integration [`Harness`] (config + store +
//! `MailService` on a temp root), a managed real-Stalwart [`StalwartFixture`]
//! for live-provider parity, and small path/port helpers.
//!
//! See `docs/testing/L1.md` for the contract this crate is the reference
//! implementation of.

mod harness;
mod paths;
mod stalwart;

pub use harness::Harness;
pub use paths::{free_loopback_port, stalwart_bin, temp_root, workspace_root};
pub use stalwart::StalwartFixture;
