//! Shared test support for Posthaste.
//!
//! Dev-only library consumed by integration tests via `[dev-dependencies]`.
//! Provides the disposable integration [`Harness`] (config + store +
//! `MailService` on a temp root), its [`Harness::with_runtime`] extension that
//! stands up an in-process authority runtime, a [`ViewSettlement`] recorder that
//! captures the ordered view-diff stream a mutation settles through, a managed
//! real-Stalwart [`StalwartFixture`] for live-provider parity, and small
//! path/port helpers.
//!
//! See `docs/testing/L1.md` for the contract this crate is the reference
//! implementation of.

mod fixture;
mod gmail;
mod harness;
mod paths;
mod replica_probe;
mod runtime;
mod stalwart;

pub use fixture::{Fixture, FixtureAccount, FixtureDriver, FixtureError, FixtureMessage};
pub use gmail::{GmailImapFixture, SEEDED_FROM_EMAIL, SEEDED_LABELS, SEEDED_SUBJECT};
pub use harness::Harness;
pub use paths::{free_loopback_port, stalwart_bin, temp_root, workspace_root};
pub use replica_probe::{FlickerLog, RenderSnapshot, RenderedRow};
pub use runtime::{FrameCapture, RuntimeHarness, TestSecretStore, ViewSettlement, ViewWatch};
pub use stalwart::StalwartFixture;
