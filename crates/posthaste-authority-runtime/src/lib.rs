//! Authority runtime implementation over local Posthaste state.
//!
//! This crate is the first extraction seam out of `posthaste-server`: it builds
//! a transport-free authority runtime handle that implements the shared runtime
//! contract without binding HTTP or creating desktop windows.
//!
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#runtime-contract-crate-first
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first

mod account_reads;
mod account_repository;
mod backend;
mod bootstrap;
mod build;
mod live_accounts;
mod mail_queries;
mod mutations;
mod near_node;
mod read;
mod transport;
pub mod oauth;
mod push;
mod secret;
mod sessions;
pub mod supervisor;
mod views;

pub use account_reads::AccountRuntimeOverviewProvider;
pub use build::{
    build_authority_runtime, AuthorityRuntimeApiMigrationBridge, AuthorityRuntimeBuild,
    AuthorityRuntimeBuildConfig, AuthorityRuntimeBuildError, AuthorityRuntimeHandle,
    AuthorityRuntimeShutdownError, BackendTransportConfig, RuntimeShutdownHandle,
};
pub use live_accounts::{LiveAccountRuntimeProvider, UnavailableLiveAccountRuntimeProvider};
pub use transport::RemoteBackend;
pub use secret::SystemSecretStore;
pub use supervisor::AccountSupervisor;
