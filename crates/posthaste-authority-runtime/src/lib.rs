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
mod link_wire;
mod local_backend;
mod mail_queries;
mod mutations;
pub mod oauth;
mod push;
mod runtime_registry;
pub mod supervisor;

// The far-node crate's own assembly surface.
pub use account_reads::AccountRuntimeOverviewProvider;
pub use build::{
    build_authority_runtime, build_backend_node, from_api_bridge_for_migration,
    from_api_bridge_with_account_supervisor_for_migration, AuthorityRuntimeApiMigrationBridge,
    AuthorityRuntimeBuild, BackendNode, MigrationRuntime,
};
pub use link_wire::{link_router, LinkAuth};
pub use live_accounts::{LiveAccountRuntimeProvider, UnavailableLiveAccountRuntimeProvider};
pub use mutations::AccountMutationService;
pub use supervisor::AccountSupervisor;

// The near node lives in `posthaste-runtime`; re-export its public surface so
// hosts (the server, benches) keep a single `posthaste_authority_runtime` import.
pub use posthaste_runtime::{
    build_remote_runtime, BackendTransportConfig, RemoteBackend, RemoteRuntimeBuild,
    RuntimeBuildConfig, RuntimeBuildError, RuntimeHandle, RuntimeShutdownError,
    RuntimeShutdownHandle, SystemSecretStore,
};
