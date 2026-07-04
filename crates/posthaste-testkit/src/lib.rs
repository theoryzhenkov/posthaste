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
mod guard;
mod harness;
mod migration;
mod paths;
mod runtime;
mod stalwart;

pub use fixture::{Fixture, FixtureAccount, FixtureDriver, FixtureError, FixtureMessage};
pub use gmail::{
    serve as serve_mock_gmail, GmailImapFixture, MAILBOX_ALL_MAIL, MAILBOX_INBOX, MAILBOX_SPAM,
    MAILBOX_STARRED, MAILBOX_TRASH, SEEDED_FROM_EMAIL, SEEDED_LABELS, SEEDED_SUBJECT,
};
pub use guard::TempDirGuard;
pub use harness::Harness;
pub use migration::runtime_handle_with_account_runtime_provider_for_migration;
pub use paths::{free_loopback_port, stalwart_bin, temp_root, workspace_root};
pub use runtime::{RuntimeHarness, TestSecretStore, ViewSettlement, ViewWatch};
pub use stalwart::StalwartFixture;
