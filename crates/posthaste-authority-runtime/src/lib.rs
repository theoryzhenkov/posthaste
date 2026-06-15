//! Authority runtime implementation over local Posthaste state.
//!
//! This crate is the first extraction seam out of `posthaste-server`: it builds
//! a transport-free authority runtime handle that implements the shared runtime
//! contract without binding HTTP or creating desktop windows.
//!
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#runtime-contract-crate-first
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first

mod bootstrap;
mod build;
mod secret;

pub use build::{
    build_authority_runtime, AuthorityRuntimeApiMigrationBridge, AuthorityRuntimeBuild,
    AuthorityRuntimeBuildConfig, AuthorityRuntimeBuildError, AuthorityRuntimeHandle,
    AuthorityRuntimeShutdownError, RuntimeShutdownHandle,
};
pub use secret::SystemSecretStore;
