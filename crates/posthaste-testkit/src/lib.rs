//! Shared test support for Posthaste.
//!
//! Dev-only library consumed by integration tests via `[dev-dependencies]`.
//! Provides the disposable integration [`Harness`] (config + store +
//! `MailService` on a temp root) with declarative TOML fixture loading, a
//! managed real-Stalwart [`StalwartFixture`] for live-provider parity, a mock
//! Gmail/generic IMAP+SMTP [`GmailImapFixture`] for provider-wire tests, an
//! in-memory [`TestSecretStore`], and small path/port helpers.
//!
//! See `docs/testing/L1.md` for the contract this crate is the reference
//! implementation of.

mod fixture;
mod gmail;
mod guard;
mod harness;
mod paths;
mod secret;
mod stalwart;

pub use fixture::{Fixture, FixtureAccount, FixtureDriver, FixtureError, FixtureMessage};
pub use gmail::{
    serve as serve_mock_gmail, GmailImapFixture, MAILBOX_ALL_MAIL, MAILBOX_DRAFTS, MAILBOX_INBOX,
    MAILBOX_SENT, MAILBOX_SPAM, MAILBOX_STARRED, MAILBOX_TRASH, SEEDED_FROM_EMAIL, SEEDED_LABELS,
    SEEDED_SUBJECT,
};
pub use guard::TempDirGuard;
pub use harness::Harness;
pub use paths::{free_loopback_port, stalwart_bin, temp_root, workspace_root};
pub use secret::TestSecretStore;
pub use stalwart::StalwartFixture;
